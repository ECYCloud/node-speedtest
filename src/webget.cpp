#include <iostream>
#include <unistd.h>
#include <sys/stat.h>
#include <mutex>

#include <curl/curl.h>

#include "webget.h"
#include "version.h"
#include "misc.h"
#include "logger.h"

#ifdef _WIN32
#ifndef _stat
#define _stat stat
#endif // _stat
#endif // _WIN32

extern bool print_debug_info, serve_cache_on_fetch_fail;
extern int global_log_level;

typedef std::lock_guard<std::mutex> guarded_mutex;
std::mutex cache_rw_lock;

// UA used for all outbound HTTP(S) requests, most importantly subscription
// fetching: many airports return a Clash/mihomo YAML only when the UA looks
// like a Clash client. Format: "Clash mihomo/<kernel-version> stairspeedtest-reborn".
// main() overrides this at startup with the REAL kernel version queried from
// the bundled mihomo binary (see buildUserAgent()); this is just the default.
std::string user_agent_str =
    "Clash mihomo/" MIHOMO_FALLBACK_VERSION " stairspeedtest-reborn";

static inline void curl_init()
{
    static bool init = false;
    if(!init)
    {
        curl_global_init(CURL_GLOBAL_ALL);
        init = true;
    }
}

static int writer(char *data, size_t size, size_t nmemb, std::string *writerData)
{
    if(writerData == NULL)
        return 0;

    writerData->append(data, size*nmemb);

    return size * nmemb;
}

static int dummy_writer(char *data, size_t size, size_t nmemb, void *writerData)
{
    /// dummy writer, do not save anything
    (void)data;
    (void)writerData;
    return size * nmemb;
}

static int size_checker(void *clientp, double dltotal, double dlnow, double ultotal, double ulnow)
{
    if(dltotal > 1048576.0)
        return 1;
    return 0;
}

static inline void curl_set_common_options(CURL *curl_handle, const char *url, long max_file_size = 1048576L)
{
    curl_easy_setopt(curl_handle, CURLOPT_URL, url);
    curl_easy_setopt(curl_handle, CURLOPT_VERBOSE, global_log_level == LOG_LEVEL_VERBOSE ? 1L : 0L);
    curl_easy_setopt(curl_handle, CURLOPT_NOPROGRESS, 0L);
    curl_easy_setopt(curl_handle, CURLOPT_NOSIGNAL, 1L);
    curl_easy_setopt(curl_handle, CURLOPT_FOLLOWLOCATION, 1L);
    curl_easy_setopt(curl_handle, CURLOPT_MAXREDIRS, 20L);
    curl_easy_setopt(curl_handle, CURLOPT_SSL_VERIFYPEER, 0L);
    curl_easy_setopt(curl_handle, CURLOPT_SSL_VERIFYHOST, 0L);
    curl_easy_setopt(curl_handle, CURLOPT_TIMEOUT, 15L);
    curl_easy_setopt(curl_handle, CURLOPT_USERAGENT, user_agent_str.data());
    if(max_file_size)
        curl_easy_setopt(curl_handle, CURLOPT_MAXFILESIZE, max_file_size);
    curl_easy_setopt(curl_handle, CURLOPT_PROGRESSFUNCTION, size_checker);
}

//static std::string curlGet(const std::string &url, const std::string &proxy, std::string &response_headers, CURLcode &return_code, const string_map &request_headers)
static int curlGet(const FetchArgument argument, FetchResult &result)
{
    CURL *curl_handle;
    std::string *data = result.content, new_url = argument.url;
    struct curl_slist *list = NULL;
    defer(curl_slist_free_all(list);)
    long retVal = 0;

    curl_init();

    curl_handle = curl_easy_init();
    if(argument.proxy.size())
    {
        if(startsWith(argument.proxy, "cors:"))
        {
            list = curl_slist_append(list, "X-Requested-With: subconverter " VERSION);
            new_url = argument.proxy.substr(5) + argument.url;
        }
        else
            curl_easy_setopt(curl_handle, CURLOPT_PROXY, argument.proxy.data());
    }
    curl_set_common_options(curl_handle, new_url.data());

    if(argument.request_headers)
    {
        for(auto &x : *argument.request_headers)
        {
            if(toLower(x.first) != "user-agent")
                list = curl_slist_append(list, (x.first + ": " + x.second).data());
        }
    }
    if(list)
        curl_easy_setopt(curl_handle, CURLOPT_HTTPHEADER, list);

    if(result.content)
    {
        curl_easy_setopt(curl_handle, CURLOPT_WRITEFUNCTION, writer);
        curl_easy_setopt(curl_handle, CURLOPT_WRITEDATA, result.content);
    }
    else
        curl_easy_setopt(curl_handle, CURLOPT_WRITEFUNCTION, dummy_writer);
    if(result.response_headers)
    {
        curl_easy_setopt(curl_handle, CURLOPT_HEADERFUNCTION, writer);
        curl_easy_setopt(curl_handle, CURLOPT_HEADERDATA, result.response_headers);
    }
    else
        curl_easy_setopt(curl_handle, CURLOPT_HEADERFUNCTION, dummy_writer);

    unsigned int fail_count = 0, max_fails = 1;
    while(true)
    {
        *result.status_code = curl_easy_perform(curl_handle);
        if(*result.status_code == CURLE_OK || max_fails >= fail_count)
            break;
        else
            fail_count++;
    }

    curl_easy_getinfo(curl_handle, CURLINFO_HTTP_CODE, &retVal);
    curl_easy_cleanup(curl_handle);

    if(data)
    {
        if(*result.status_code != CURLE_OK || retVal != 200)
            data->clear();
        data->shrink_to_fit();
    }

    return *result.status_code;
}

// data:[<mediatype>][;base64],<data>
static std::string dataGet(const std::string &url)
{
    if (!startsWith(url, "data:"))
        return std::string();
    std::string::size_type comma = url.find(',');
    if (comma == std::string::npos || comma == url.size() - 1)
        return std::string();

    std::string data = UrlDecode(url.substr(comma + 1));
    if (endsWith(url.substr(0, comma), ";base64")) {
        return urlsafe_base64_decode(data);
    } else {
        return data;
    }
}

std::string buildSocks5ProxyString(const std::string &addr, int port, const std::string &username, const std::string &password)
{
    std::string authstr = username.size() && password.size() ? username + ":" + password + "@" : "";
    std::string proxystr = "socks5://" + authstr + addr + ":" + std::to_string(port);
    return proxystr;
}

// Measure real request latency to `url` through `proxy` (a socks5:// string).
//
// HTTPS includes a TLS handshake that would inflate a single measurement, so we
// REUSE one libcurl handle: a throwaway warm-up request first establishes the
// TCP+TLS connection, then `probes` measured requests run on the warm (kept-
// alive) connection — their CURLINFO_TOTAL_TIME excludes the handshake. We
// return the mean of the successful measured probes (ms), or -1.0 if all fail.
// Per-probe ms (0 = failed) go into raw[0..probes-1] when non-null. When
// `progress_label` is non-empty we draw a live progress bar to stderr.
//
// 超时给 20s(原来的 10s 太紧):Hysteria2 / TUIC 等 QUIC 协议首次握手包要走完整
// 1-RTT 链路，加上代理转发，在弱网下 10s 内 warmup+probe 几乎一定会超时，
// 表现就是测速能跑(单个长连接已建立)，但延迟列全 N/A。
double measureLatency(const std::string &url, const std::string &proxy,
                      int probes, int *raw, const std::string &progress_label)
{
    curl_init();
    CURL *h = curl_easy_init();
    if(!h) return -1.0;
    curl_easy_setopt(h, CURLOPT_URL, url.data());
    if(proxy.size())
        curl_easy_setopt(h, CURLOPT_PROXY, proxy.data());
    curl_easy_setopt(h, CURLOPT_NOSIGNAL, 1L);
    curl_easy_setopt(h, CURLOPT_NOPROGRESS, 1L);
    curl_easy_setopt(h, CURLOPT_FOLLOWLOCATION, 1L);
    curl_easy_setopt(h, CURLOPT_MAXREDIRS, 5L);
    curl_easy_setopt(h, CURLOPT_SSL_VERIFYPEER, 0L);
    curl_easy_setopt(h, CURLOPT_SSL_VERIFYHOST, 0L);
    curl_easy_setopt(h, CURLOPT_CONNECTTIMEOUT, 20L);
    curl_easy_setopt(h, CURLOPT_TIMEOUT, 20L);
    curl_easy_setopt(h, CURLOPT_USERAGENT, user_agent_str.data());
    curl_easy_setopt(h, CURLOPT_WRITEFUNCTION, dummy_writer);
    curl_easy_setopt(h, CURLOPT_FORBID_REUSE, 0L); // allow keep-alive reuse

    bool show = !progress_label.empty();
    if(show)
        std::cerr << progress_label << " ";

    // Warm-up: pays the TCP + TLS handshake once; result discarded.
    // 失败也不要紧，后续 probe 会重新拨号。
    CURLcode warm = curl_easy_perform(h);
    if(warm != CURLE_OK)
        writeLog(LOG_TYPE_WARN, std::string("Latency warmup failed (")
                 + curl_easy_strerror(warm) + ") url=" + url + " proxy=" + proxy);

    double total = 0.0;
    int ok = 0;
    for(int i = 0; i < probes; ++i)
    {
        if(raw) raw[i] = 0;
        CURLcode res = curl_easy_perform(h);
        int ms = 0;
        if(res == CURLE_OK)
        {
            double t = 0.0;
            curl_easy_getinfo(h, CURLINFO_TOTAL_TIME, &t);
            ms = static_cast<int>(t * 1000.0 + 0.5);
            if(ms <= 0) ms = 1;
            if(raw) raw[i] = ms;
            total += ms;
            ok++;
        }
        else
        {
            // 把 libcurl 错误码记下来，便于排查"为什么 hy2 / reality 节点延迟全 N/A"。
            writeLog(LOG_TYPE_WARN, std::string("Latency probe ") + std::to_string(i + 1)
                     + "/" + std::to_string(probes) + " failed: " + curl_easy_strerror(res)
                     + " url=" + url);
        }
        if(show)
            std::cerr << (res == CURLE_OK ? "[" + std::to_string(ms) + "ms]"
                                          : "[超时]")
                      << " " << (i + 1) << "/" << probes << "  " << std::flush;
    }
    curl_easy_cleanup(h);
    if(show)
        std::cerr << std::endl;
    if(ok == 0) return -1.0;
    return total / ok;
}

std::string webGet(const std::string &url, const std::string &proxy, unsigned int cache_ttl, std::string *response_headers, string_map *request_headers)
{
    int return_code = 0;
    std::string content;

    FetchArgument argument {url, proxy, request_headers, cache_ttl};
    FetchResult fetch_res {&return_code, &content, response_headers};

    if (startsWith(url, "data:"))
        return dataGet(url);
    // cache system
    if(cache_ttl > 0)
    {
        md("cache");
        const std::string url_md5 = getMD5(url);
        const std::string path = "cache/" + url_md5, path_header = path + "_header";
        struct stat result;
        if(stat(path.data(), &result) == 0) // cache exist
        {
            time_t mtime = result.st_mtime, now = time(NULL); // get cache modified time and current time
            if(difftime(now, mtime) <= cache_ttl) // within TTL
            {
                writeLog(0, "CACHE HIT: '" + url + "', using local cache.");
                guarded_mutex guard(cache_rw_lock);
                if(response_headers)
                    *response_headers = fileGet(path_header, true);
                return fileGet(path, true);
            }
            writeLog(0, "CACHE MISS: '" + url + "', TTL timeout, creating new cache."); // out of TTL
        }
        else
            writeLog(0, "CACHE NOT EXIST: '" + url + "', creating new cache.");
        //content = curlGet(url, proxy, response_headers, return_code); // try to fetch data
        curlGet(argument, fetch_res);
        if(return_code == CURLE_OK) // success, save new cache
        {
            guarded_mutex guard(cache_rw_lock);
            fileWrite(path, content, true);
            if(response_headers)
                fileWrite(path_header, *response_headers, true);
        }
        else
        {
            if(fileExist(path) && serve_cache_on_fetch_fail) // failed, check if cache exist
            {
                writeLog(0, "Fetch failed. Serving cached content."); // cache exist, serving cache
                guarded_mutex guard(cache_rw_lock);
                content = fileGet(path, true);
                if(response_headers)
                    *response_headers = fileGet(path_header, true);
            }
            else
                writeLog(0, "Fetch failed. No local cache available."); // cache not exist or not allow to serve cache, serving nothing
        }
        return content;
    }
    //return curlGet(url, proxy, response_headers, return_code);
    curlGet(argument, fetch_res);
    return content;
}

int curlPost(const std::string &url, const std::string &data, const std::string &proxy, const string_array &request_headers, std::string *retData)
{
    CURL *curl_handle;
    CURLcode res;
    struct curl_slist *list = NULL;
    long retVal = 0;

    curl_init();
    curl_handle = curl_easy_init();
    list = curl_slist_append(list, "Content-Type: application/json;charset='utf-8'");
    for(const std::string &x : request_headers)
        list = curl_slist_append(list, x.data());

    curl_set_common_options(curl_handle, url.data(), 0L);
    curl_easy_setopt(curl_handle, CURLOPT_POST, 1L);
    curl_easy_setopt(curl_handle, CURLOPT_POSTFIELDS, data.data());
    curl_easy_setopt(curl_handle, CURLOPT_POSTFIELDSIZE, data.size());
    curl_easy_setopt(curl_handle, CURLOPT_WRITEFUNCTION, writer);
    curl_easy_setopt(curl_handle, CURLOPT_WRITEDATA, retData);
    curl_easy_setopt(curl_handle, CURLOPT_HTTPHEADER, list);
    if(proxy.size())
        curl_easy_setopt(curl_handle, CURLOPT_PROXY, proxy.data());

    res = curl_easy_perform(curl_handle);
    curl_slist_free_all(list);

    if(res == CURLE_OK)
    {
        res = curl_easy_getinfo(curl_handle, CURLINFO_HTTP_CODE, &retVal);
    }

    curl_easy_cleanup(curl_handle);

    return retVal;
}

int webPost(const std::string &url, const std::string &data, const std::string &proxy, const string_array &request_headers, std::string *retData)
{
    return curlPost(url, data, proxy, request_headers, retData);
}

int curlPatch(const std::string &url, const std::string &data, const std::string &proxy, const string_array &request_headers, std::string *retData)
{
    CURL *curl_handle;
    CURLcode res;
    long retVal = 0;
    struct curl_slist *list = NULL;

    curl_init();

    curl_handle = curl_easy_init();

    list = curl_slist_append(list, "Content-Type: application/json;charset='utf-8'");
    for(const std::string &x : request_headers)
        list = curl_slist_append(list, x.data());

    curl_set_common_options(curl_handle, url.data(), 0L);
    curl_easy_setopt(curl_handle, CURLOPT_CUSTOMREQUEST, "PATCH");
    curl_easy_setopt(curl_handle, CURLOPT_POSTFIELDS, data.data());
    curl_easy_setopt(curl_handle, CURLOPT_POSTFIELDSIZE, data.size());
    curl_easy_setopt(curl_handle, CURLOPT_WRITEFUNCTION, writer);
    curl_easy_setopt(curl_handle, CURLOPT_WRITEDATA, retData);
    curl_easy_setopt(curl_handle, CURLOPT_HTTPHEADER, list);
    if(proxy.size())
        curl_easy_setopt(curl_handle, CURLOPT_PROXY, proxy.data());

    res = curl_easy_perform(curl_handle);
    curl_slist_free_all(list);
    if(res == CURLE_OK)
    {
        res = curl_easy_getinfo(curl_handle, CURLINFO_HTTP_CODE, &retVal);
    }

    curl_easy_cleanup(curl_handle);

    return retVal;
}

int webPatch(const std::string &url, const std::string &data, const std::string &proxy, const string_array &request_headers, std::string *retData)
{
    return curlPatch(url, data, proxy, request_headers, retData);
}

// HTTP PUT — used to drive the Clash API (e.g. PUT /proxies/GLOBAL to switch
// the active outbound). Identical to curlPatch except for CUSTOMREQUEST.
int curlPut(const std::string &url, const std::string &data, const std::string &proxy, const string_array &request_headers, std::string *retData)
{
    CURL *curl_handle;
    CURLcode res;
    long retVal = 0;
    struct curl_slist *list = NULL;

    curl_init();
    curl_handle = curl_easy_init();
    list = curl_slist_append(list, "Content-Type: application/json;charset='utf-8'");
    for(const std::string &x : request_headers)
        list = curl_slist_append(list, x.data());

    curl_set_common_options(curl_handle, url.data(), 0L);
    curl_easy_setopt(curl_handle, CURLOPT_CUSTOMREQUEST, "PUT");
    curl_easy_setopt(curl_handle, CURLOPT_POSTFIELDS, data.data());
    curl_easy_setopt(curl_handle, CURLOPT_POSTFIELDSIZE, data.size());
    curl_easy_setopt(curl_handle, CURLOPT_WRITEFUNCTION, writer);
    curl_easy_setopt(curl_handle, CURLOPT_WRITEDATA, retData);
    curl_easy_setopt(curl_handle, CURLOPT_HTTPHEADER, list);
    if(proxy.size())
        curl_easy_setopt(curl_handle, CURLOPT_PROXY, proxy.data());

    res = curl_easy_perform(curl_handle);
    curl_slist_free_all(list);
    if(res == CURLE_OK)
        res = curl_easy_getinfo(curl_handle, CURLINFO_HTTP_CODE, &retVal);
    curl_easy_cleanup(curl_handle);
    return retVal;
}

int webPut(const std::string &url, const std::string &data, const std::string &proxy, const string_array &request_headers, std::string *retData)
{
    return curlPut(url, data, proxy, request_headers, retData);
}

int curlHead(const std::string &url, const std::string &proxy, const string_array &request_headers, std::string &response_headers)
{
    CURL *curl_handle;
    CURLcode res;
    long retVal = 0;
    struct curl_slist *list = NULL;

    curl_init();

    curl_handle = curl_easy_init();

    list = curl_slist_append(list, "Content-Type: application/json;charset='utf-8'");
    for(const std::string &x : request_headers)
        list = curl_slist_append(list, x.data());

    curl_set_common_options(curl_handle, url.data());
    curl_easy_setopt(curl_handle, CURLOPT_HEADERFUNCTION, writer);
    curl_easy_setopt(curl_handle, CURLOPT_HEADERDATA, &response_headers);
    curl_easy_setopt(curl_handle, CURLOPT_NOBODY, 1L);
    curl_easy_setopt(curl_handle, CURLOPT_HTTPHEADER, list);
    if(proxy.size())
        curl_easy_setopt(curl_handle, CURLOPT_PROXY, proxy.data());

    res = curl_easy_perform(curl_handle);
    curl_slist_free_all(list);
    if(res == CURLE_OK)
        res = curl_easy_getinfo(curl_handle, CURLINFO_HTTP_CODE, &retVal);

    curl_easy_cleanup(curl_handle);

    return retVal;
}

int webHead(const std::string &url, const std::string &proxy, const string_array &request_headers, std::string &response_headers)
{
    return curlHead(url, proxy, request_headers, response_headers);
}
