#include <string>
#include <map>
#include <vector>
#include <chrono>
#include <iostream>
#include <regex>
#include <fstream>
#include <iomanip>
#include <algorithm>
#include <numeric>
#include <thread>
#include <csignal>

#ifdef _WIN32
#include <conio.h>
#else
#include <termios.h>
#endif // _WIN32

#include "socket.h"
#include "misc.h"
#include "speedtestutil.h"
#include "subconv.h"
#include "printout.h"
#include "printmsg.h"
#include "webget.h"
#include "logger.h"
#include "renderer.h"
#include "processes.h"
#include "rulematch.h"
#include "version.h"
#include "ini_reader.h"
#include "multithread_test.h"
#include "nodeinfo.h"
#include "rapidjson_extra.h"

using namespace std::chrono;

#define MAX_FILE_SIZE 100 * 1024 * 1024

//use for command argument
bool rpcmode = false;
std::string sub_url;
bool pause_on_done = true;

#ifdef BUILD_WEBSERVER_ENGINE
#include <atomic>
// Webserver 引擎模式(供 Tauri 桌面端 sidecar 使用):/web 启动后通过 HTTP 接口
// 暴露 /readsubscriptions /start /getresults 等路由,由 webgui_wrapper.cpp 实现。
// CLI 构建(默认)下整个 BUILD_WEBSERVER_ENGINE 段都不编译,二进制保持纯净。
bool webserver_mode = false;
std::string listen_address = "127.0.0.1";
int listen_port = 10870;
void ssrspeed_webserver_routine(const std::string &listen_address, int listen_port);
// /stop 路由把它置 true → batchTest 节点循环间检查 → 跳出 → 保留已测结果。
// /start 启动新一轮时复位。CLI 模式根本编不到 batchTest 里的检查点,无副作用。
extern std::atomic<bool> stop_requested;
#endif

//for use globally
bool multilink = false;
int socksport = 65432;
std::string socksaddr = "127.0.0.1";
std::string custom_group;
std::string pngpath;

//current node id being tested (shared with result serialization)
int cur_node_id = -1;

bool ss_libev = true;
bool ssr_libev = true;
std::string def_test_file = "https://speed.cloudflare.com/__down?bytes=95000000";
std::string def_upload_target = "http://losangeles.speed.googlefiber.net:3004/upload?time=0";
std::vector<downloadLink> downloadFiles;
std::vector<linkMatchRule> matchRules;
string_array custom_exclude_remarks, custom_include_remarks, dict, trans;
std::vector<nodeInfo> allNodes;
std::string speedtest_mode = "all";
std::string override_conf_port = "";
int def_thread_count = 4;
bool export_with_maxspeed = true;
bool export_as_new_style = true;
bool test_site_ping = true;
bool test_upload = false;
bool test_nat_type = true;
bool multilink_export_as_one_image = false;
bool single_test_force_export = false;
bool verbose = false;
std::string export_sort_method = "rmaxspeed";

// Single flag: whether the mihomo binary exists and the kernel can be used.
// Every node now routes through the kernel, so a per-protocol matrix is gone.
bool mihomo_available = false;
unsigned int node_count = 0;
int curGroupID = 0;

int global_log_level = LOG_LEVEL_ERROR;
bool serve_cache_on_fetch_fail = false, print_debug_info = false;

//declarations

int explodeLog(const std::string &log, std::vector<nodeInfo> &nodes);
void getTestFile(nodeInfo &node, const std::string &proxy, const std::vector<downloadLink> &downloadFiles, const std::vector<linkMatchRule> &matchRules, const std::string &defaultTestFile);
std::string get_nat_type_thru_socks5(const std::string &server, uint16_t port, const std::string &username = "", const std::string &password = "", const std::string &stun_server = "stun.l.google.com", uint16_t stun_port = 19302);
// Forward declaration so the signal handler (defined early in this file) can
// reuse the same label-decoration pass that batchTest() runs at the end of a
// normal run.
static void decorateNodeLabels(std::vector<nodeInfo> &nodes);

//original codes

#ifndef _WIN32

int _getch()
{
    int ch;
    struct termios tm, tm_old;
    int fd = 0;

    if (tcgetattr(fd, &tm) < 0)
    {
        return -1;
    }

    tm_old = tm;
    cfmakeraw(&tm);
    if (tcsetattr(fd, TCSANOW, &tm) < 0)
    {
        return -1;
    }

    ch = std::cin.get();
    if (tcsetattr(fd, TCSANOW, &tm_old) < 0)
    {
        return -1;
    }
    return ch;
}


void SetConsoleTitle(std::string title)
{
    system(std::string("echo \"\\033]0;" + title + "\\007\\c\"").data());
}

#endif // _WIN32

void clearTrans()
{
    eraseElements(dict);
    eraseElements(trans);
}

void addTrans(std::string dictval, std::string transval)
{
    dict.push_back(dictval);
    trans.push_back(transval);
}

void copyNodes(std::vector<nodeInfo> &source, std::vector<nodeInfo> &dest)
{
    for(auto &x : source)
    {
        dest.push_back(x);
    }
}

void copyNodesWithGroupID(std::vector<nodeInfo> &source, std::vector<nodeInfo> &dest, int groupID)
{
    for(auto &x : source)
    {
        if(x.groupID == groupID)
            dest.push_back(x);
    }
}

void clientCheck()
{
#ifdef _WIN32
    std::string mihomo_path = "tools\\clients\\mihomo.exe";
#else
    std::string mihomo_path = "tools/clients/mihomo";
#endif // _WIN32

    mihomo_available = fileExist(mihomo_path);
    if(mihomo_available)
        writeLog(LOG_TYPE_INFO, "Found mihomo core at path " + mihomo_path);
    else
        writeLog(LOG_TYPE_WARN, "mihomo core not found at path " + mihomo_path);
}

// Build the subscription User-Agent as "Clash mihomo/<ver> stairspeedtest-reborn",
// querying the REAL kernel version from `mihomo -v` so it never goes stale.
// Falls back to MIHOMO_FALLBACK_VERSION if the binary can't be run/parsed.
// Overrides the global `user_agent_str` defined in webget.cpp.
extern std::string user_agent_str;
extern std::string mihomo_kernel_version; // defined in renderer.cpp (footer)
static void initUserAgent()
{
#ifdef _WIN32
    const char *cmd = "tools\\clients\\mihomo.exe -v";
#else
    const char *cmd = "tools/clients/mihomo -v";
#endif
    // 用静默捕获(CreateProcess + CREATE_NO_WINDOW)取版本，避免 _popen 弹 cmd 窗口
    std::string ver = MIHOMO_FALLBACK_VERSION;
    std::string out = runCommandCapture(cmd);
    if(!out.empty())
    {
        // Output looks like: "Mihomo Meta vX.Y.Z windows amd64 with go..."
        std::string::size_type v = out.find(" v");
        if(v != std::string::npos)
        {
            std::string::size_type s = v + 1;       // points at 'v'
            std::string::size_type e = s;
            while(e < out.size() && out[e] != ' ' && out[e] != '\r' && out[e] != '\n')
                ++e;
            std::string token = out.substr(s, e - s);
            if(token.size() >= 2 && token[0] == 'v')
                ver = token;
        }
    }
    user_agent_str = "Clash mihomo/" + ver + " stairspeedtest-reborn";
    mihomo_kernel_version = ver; // share the real version with the footer
    writeLog(LOG_TYPE_INFO, "Subscription User-Agent: " + user_agent_str);
}

// Compare two "vX.Y.Z" version strings segment-by-segment as integers.
// Returns >0 if a>b, <0 if a<b, 0 if equal. Missing segments count as 0, so
// "v1.19" vs "v1.19.0" compares equal and "v1.9" < "v1.19" (numeric, not lex).
// Non-static so the webserver engine (webgui_wrapper.cpp) can reuse it for
// the /checkupdate route — version compare is otherwise duplicated logic.
int compareKernelVersion(const std::string &a, const std::string &b)
{
    auto strip = [](std::string v){ if(!v.empty() && (v[0] == 'v' || v[0] == 'V')) v.erase(0, 1); return v; };
    string_array sa = split(strip(a), "."), sb = split(strip(b), ".");
    size_t n = std::max(sa.size(), sb.size());
    for(size_t i = 0; i < n; ++i)
    {
        int x = 0, y = 0;
        try { if(i < sa.size()) x = std::stoi(sa[i]); } catch(...) { x = 0; }
        try { if(i < sb.size()) y = std::stoi(sb[i]); } catch(...) { y = 0; }
        if(x != y) return x - y;
    }
    return 0;
}

// Query the latest mihomo kernel release from GitHub and, if it is newer than
// the bundled kernel, print a one-line hint. Best-effort: any network/parse
// failure is logged and silently ignored (no UI noise). Runs in a detached
// background thread so a slow/blocked GitHub connection never stalls startup.
static void checkMihomoUpdate()
{
    std::thread([](){
        const std::string local = mihomo_kernel_version; // set by initUserAgent()
        if(local.empty()) return;
        // /releases/latest returns the latest non-prerelease; tag_name is "vX.Y.Z".
        std::string body = webGet("https://api.github.com/repos/MetaCubeX/mihomo/releases/latest", "", 0);
        if(body.empty())
        {
            writeLog(LOG_TYPE_WARN, "mihomo update check: empty response from GitHub.");
            return;
        }
        rapidjson::Document j;
        j.Parse(body.data());
        if(j.HasParseError() || !j.HasMember("tag_name") || !j["tag_name"].IsString())
        {
            writeLog(LOG_TYPE_WARN, "mihomo update check: unexpected GitHub response.");
            return;
        }
        std::string latest = j["tag_name"].GetString();
        if(latest.empty()) return;
        if(compareKernelVersion(latest, local) > 0)
        {
            writeLog(LOG_TYPE_INFO, "mihomo update available: local " + local + ", latest " + latest);
            std::cout << "\n发现新版 mihomo 内核：本地 " << local << "，最新 " << latest
                      << "。\n下载：https://github.com/MetaCubeX/mihomo/releases/latest\n" << std::endl;
        }
        else
            writeLog(LOG_TYPE_INFO, "mihomo kernel is up to date (" + local + ").");
    }).detach();
}

int runClient(int client)
{
    (void)client;
#ifdef _WIN32
    std::string mihomo_cmd = "tools\\clients\\mihomo.exe -d . -f config.yaml";
#else
    std::string mihomo_cmd = "tools/clients/mihomo -d . -f config.yaml";
#endif // _WIN32
    writeLog(LOG_TYPE_INFO, "Starting up mihomo core...");
    runProgram(mihomo_cmd, "", false);
    return 0;
}

// Block until the mihomo Clash API answers /version, or timeout. Returns true
// when the kernel is ready to accept proxy switches.
bool mihomoWaitReady(int timeout_ms = 5000)
{
    std::string ret;
    int elapsed = 0;
    const int step = 100;
    while(elapsed < timeout_ms)
    {
        ret.clear();
        long code = webPost("http://127.0.0.1:9990/version", "", "", string_array{}, &ret);
        // mihomo responds 405 to POST on /version but the connection itself
        // confirms the API is up; any non-zero HTTP code means alive.
        // We try a real GET via webGet first, falling back to socket probe.
        std::string body = webGet("http://127.0.0.1:9990/version", "", 0);
        if(!body.empty() && body.find("\"version\"") != std::string::npos)
            return true;
        (void)code;
        sleep(step);
        elapsed += step;
    }
    return false;
}

// 轻量探活:连一下 127.0.0.1:9990,连得上即视为内核还在监听 Clash API。
// 故意不发 HTTP 请求,避免被 mihomo 假死时长时间 hang;用 1.5s 的连接超时
// 触发立刻失败,避免拖慢测试节奏。
//
// 已知局限:仅探测 TCP 端口可达,不探 mihomo goroutine 状态。"端口在但内核
// 半死"的极端情况下会漏检 — 这种情况下 SwitchProxy 会失败,旧 outbound 上
// 的下载也会失败,与"进程不在"等价 — 触发外层 ensureAlive 同样会重启。
bool mihomoIsAlive()
{
    SOCKET s = initSocket(getNetworkType("127.0.0.1"), SOCK_STREAM, IPPROTO_TCP);
    if(s == INVALID_SOCKET) return false;
    setTimeout(s, 1500);
    int rc = startConnect(s, "127.0.0.1", 9990);
    closesocket(s);
    return rc != SOCKET_ERROR;
}

// 节点测试前的健康守护:mihomo 内核中途崩溃后,后面所有节点都会因 9990 / 65432
// 拒连而 N/A。用户视角就是"测着测着后面节点全测不出"。
// 这里在每个节点开测前调用一次,死了就用既有的 config.yaml + provider 重新拉起,
// outbound 列表自动恢复,本次测试可以无缝续跑。
//
// 设计原则:
//  * 探活只 ~1ms(一次本地 TCP connect),不影响正常节奏
//  * 死了只重启一次;重启失败就放弃,让本节点跑完拿 N/A 而不是死循环
//  * 不动 yaml/provider 文件,不重新 buildProvidersConfig — 那已是上一轮 launch 写好的
bool mihomoEnsureAlive()
{
    if(mihomoIsAlive()) return true;
    writeLog(LOG_TYPE_WARN, "mihomo Clash API 不可达,内核可能已退出,尝试重启...");
#ifdef _WIN32
    killProgram("mihomo.exe");
#else
    killProgram("mihomo");
#endif
    runClient(0);
    bool ok = mihomoWaitReady(8000);
    if(ok)
        writeLog(LOG_TYPE_INFO, "mihomo 重启完成,继续测试。");
    else
        writeLog(LOG_TYPE_ERROR, "mihomo 重启后 8s 内仍未就绪,本节点将取得 N/A。");
    return ok;
}

// Switch the GLOBAL selector to the given outbound name. Returns true on 2xx.
bool mihomoSwitchProxy(const std::string &name)
{
    std::string body = "{\"name\":\"" + name + "\"}";
    std::string resp;
    long code = webPut("http://127.0.0.1:9990/proxies/GLOBAL", body, "", string_array{}, &resp);
    if(code >= 200 && code < 300)
        return true;
    writeLog(LOG_TYPE_ERROR, "Failed to switch proxy to '" + name + "' (HTTP " + std::to_string(code) + "): " + resp);
    return false;
}

// 首选 mihomo Clash API /proxies/<name>/delay:与 Clash Verge / Karing 等
// 客户端同源,unified-delay 模式下数字可比、绝对值贴近真实 RTT。
// 端点形如 http://127.0.0.1:9990/proxies/<urlencoded-name>/delay?url=...&timeout=8000
static bool mihomoMeasureDelay(const std::string &proxy_name, int *out_ms,
                               const std::string &url = "https://cp.cloudflare.com/generate_204",
                               int timeout_ms = 8000)
{
    std::string ep = "http://127.0.0.1:9990/proxies/" + UrlEncode(proxy_name) +
                     "/delay?timeout=" + std::to_string(timeout_ms) +
                     "&url=" + UrlEncode(url);
    std::string body = webGet(ep, "", 0);
    if(body.empty()) return false;
    rapidjson::Document j;
    j.Parse(body.data());
    if(j.HasParseError()) return false;
    if(!j.HasMember("delay") || !j["delay"].IsNumber()) return false;
    int v = static_cast<int>(j["delay"].GetDouble());
    if(v <= 0) return false;
    if(out_ms) *out_ms = v;
    return true;
}

// 兜底:mihomo /delay 失败(常见于 hy2/anytls 等 QUIC 节点对内核内置 HEAD 不友好,
// 或测试 URL 被节点出口运营商拦截)时,改走 libcurl 经 mihomo socks5 自己发 HEAD。
// 口径不同 — 含一次完整拨号+握手+HEAD 的 wall clock,数字会偏高,但比显示 N/A 好,
// 用户至少能看到节点是"通的、慢" vs "完全不通"的差异。
static bool socks5MeasureDelay(const std::string &socks_addr, int socks_port,
                               int *out_ms,
                               const std::string &url = "https://cp.cloudflare.com/generate_204")
{
    std::string proxy = buildSocks5ProxyString(socks_addr, socks_port, "", "");
    std::string headers;
    auto t0 = std::chrono::steady_clock::now();
    int code = webHead(url, proxy, string_array{}, headers);
    auto t1 = std::chrono::steady_clock::now();
    if(code < 200 || code >= 400) return false;
    int ms = static_cast<int>(std::chrono::duration_cast<std::chrono::milliseconds>(t1 - t0).count());
    if(ms <= 0) return false;
    if(out_ms) *out_ms = ms;
    return true;
}

int killClient(int client)
{
    (void)client;
#ifdef _WIN32
    std::string mihomo_name = "mihomo.exe";
#else
    std::string mihomo_name = "mihomo";
#endif // _WIN32
    writeLog(LOG_TYPE_INFO, "Killing mihomo core...");
    killProgram(mihomo_name);
    return 0;
}

// 启动后从内核反查某个 provider 的节点列表，按写入顺序回填权威 name(用于切
// outbound)和 type(用于显示)。内核对 YAML proxies 数组与 base64 链接列表都按
// 行序保序，所以直接 zip;若数量不一致(内核丢弃了非法节点)取较小长度对齐。
static void reconcileProvider(const std::string &provider,
                              std::vector<nodeInfo*> &order)
{
    if(order.empty()) return;
    std::string body = webGet("http://127.0.0.1:9990/providers/proxies/" +
                              UrlEncode(provider), "", 0);
    if(body.empty()) return;
    rapidjson::Document j;
    j.Parse(body.data());
    if(j.HasParseError() || !j.HasMember("proxies") || !j["proxies"].IsArray())
        return;
    const auto &arr = j["proxies"];
    size_t n = std::min(static_cast<size_t>(arr.Size()), order.size());
    for(size_t i = 0; i < n; ++i)
    {
        const auto &p = arr[static_cast<rapidjson::SizeType>(i)];
        if(p.HasMember("name") && p["name"].IsString())
            order[i]->proxyStr = p["name"].GetString();
        if(p.HasMember("type") && p["type"].IsString())
            order[i]->proxy_type = p["type"].GetString();
    }
}

// 启动 mihomo 内核:把存活节点拆成 file proxy-provider(YAML 单元 + 链接单元),
// 写盘后起内核;内核自己解析协议字段,新协议跟着内核走无需改 C++。就绪后反查
// /providers/proxies 回填每个节点权威的 name(切 outbound 用)和 type(显示用)。
//
// 返回 true 表示 Clash API 已就绪可测试;false 表示无可测节点或启动超时。
bool launchMihomoForNodes(std::vector<nodeInfo> &nodes)
{
    if(nodes.empty()) return false;
    if(!mihomo_available) return false;

    ProvidersBuild build = buildProvidersConfig(nodes, socksport, "127.0.0.1:9990");
    if(build.yaml_order.empty() && build.link_order.empty())
        return false; // 全是 LOG 导入或空节点,无需内核

    // 先杀掉旧的 mihomo，避免 9990 / 端口冲突
#ifdef _WIN32
    killProgram("mihomo.exe");
#else
    killProgram("mihomo");
#endif

    if(!build.yaml_provider_path.empty())
        fileWrite(build.yaml_provider_path, build.yaml_provider_body, true);
    if(!build.link_provider_path.empty())
        fileWrite(build.link_provider_path, build.link_provider_body, true);
    fileWrite("config.yaml", build.config_yaml, true);
    writeLog(LOG_TYPE_INFO, "Wrote provider files + config.yaml for " +
             std::to_string(build.yaml_order.size() + build.link_order.size()) +
             " nodes; starting mihomo once.");
    runClient(0);
    if(!mihomoWaitReady(8000))
    {
        writeLog(LOG_TYPE_ERROR, "mihomo did not respond on http://127.0.0.1:9990 within 8s; tests may fail.");
        return false;
    }
    writeLog(LOG_TYPE_INFO, "mihomo Clash API is ready; reconciling node names/types.");
    reconcileProvider("yaml_sub", build.yaml_order);
    reconcileProvider("link_sub", build.link_order);
    return true;
}

int terminateClient(int client)
{
    killByHandle();
#ifdef __APPLE__
    killClient(client);
#endif // __APPLE__
    return 0;
}

void readConf(std::string path)
{
    downloadLink link;
    linkMatchRule rule;
    unsigned int i;
    string_array vChild, vArray;
    INIReader ini;
    std::string strTemp;

    //ini.do_utf8_to_gbk = true;
    ini.ParseFile(path);

    ini.EnterSection("common");
    if(ini.ItemPrefixExist("exclude_remark"))
        ini.GetAll("exclude_remark", custom_exclude_remarks);
    if(ini.ItemPrefixExist("include_remark"))
        ini.GetAll("include_remark", custom_include_remarks);

    ini.EnterSection("advanced");
    ini.GetIfExist("speedtest_mode", speedtest_mode);
    ini.GetBoolIfExist("test_site_ping", test_site_ping);
    ini.GetBoolIfExist("test_upload", test_upload);
    ini.GetBoolIfExist("test_nat_type", test_nat_type);
    // preferred_ss_client / preferred_ssr_client INI keys are now ignored: the
    // mihomo single-kernel build does not use ss-csharp / ssr-csharp anymore.
    ini.GetIfExist("override_conf_port", override_conf_port);
    ini.GetIntIfExist("thread_count", def_thread_count);
    ini.GetBoolIfExist("pause_on_done", pause_on_done);

    ini.EnterSection("export");
    ini.GetBoolIfExist("export_with_maxspeed", export_with_maxspeed);
    ini.GetIfExist("export_sort_method", export_sort_method);
    ini.GetBoolIfExist("multilink_export_as_one_image", multilink_export_as_one_image);
    ini.GetBoolIfExist("single_test_force_export", single_test_force_export);
    ini.GetBoolIfExist("export_as_new_style", export_as_new_style);
    ini.GetIntIfExist("image_scale", image_scale);
    if(image_scale < 1) image_scale = 1;
    if(image_scale > 6) image_scale = 6;
    ini.GetBoolIfExist("export_as_ssrspeed", export_as_ssrspeed);

#ifdef BUILD_WEBSERVER_ENGINE
    // [webserver] 段:仅引擎构建模式下读取,允许 pref.ini 覆盖默认监听地址/端口。
    ini.EnterSection("webserver");
    ini.GetIfExist("listen_address", listen_address);
    ini.GetIntIfExist("listen_port", listen_port);
    ini.GetBoolIfExist("webserver_mode", webserver_mode);
#endif

    ini.EnterSection("rules");
    if(ini.ItemPrefixExist("test_file_urls"))
    {
        eraseElements(vArray);
        ini.GetAll("test_file_urls", vArray);
        for(auto &x : vArray)
        {
            vChild = split(x, "|");
            if(vChild.size() == 2)
            {
                link.url = vChild[0];
                link.tag = vChild[1];
                downloadFiles.push_back(link);
            }
        }
    }
    if(ini.ItemPrefixExist("rules"))
    {
        eraseElements(vArray);
        ini.GetAll("rules", vArray);
        for(auto &x : vArray)
        {
            vChild = split(x, "|");
            if(vChild.size() >= 3)
            {
                eraseElements(rule.rules);
                rule.mode = vChild[0];
                for(i = 1; i < vChild.size() - 1; i++)
                {
                    rule.rules.push_back(vChild[i]);
                }
                rule.tag = vChild[vChild.size() - 1];
                matchRules.push_back(rule);
            }
        }
    }
}

extern std::vector<nodeInfo> allNodes;
void saveResult(std::vector<nodeInfo> &nodes);

// renderer.cpp 里定义的全局，batchTest 开始时填充它们供 PNG footer 使用
extern std::string g_local_country, g_local_region, g_local_city, g_local_isp;
extern std::string g_test_start_time, g_test_tz_label;

// ip-api.com 返回的 ISP 字段是英文(China Telecom backbone / Chinanet 等),
// 这里做关键词→中文映射，避免在结果图脚部和前端展示一长串英文。
// 没匹配的保留原文(便于排查)，不破坏可读性。
static std::string translateIsp(const std::string &raw)
{
    if(raw.empty()) return raw;
    std::string lower = toLower(raw);
    struct Rule { const char *kw; const char *zh; };
    // 顺序敏感:更长更精确的关键词先匹配
    static const Rule rules[] = {
        {"chinanet",          "中国电信"},
        {"china telecom",     "中国电信"},
        {"chinaunicom",       "中国联通"},
        {"china unicom",      "中国联通"},
        {"china169",          "中国联通"},
        {"unicom",            "中国联通"},
        {"chinamobile",       "中国移动"},
        {"china mobile",      "中国移动"},
        {"cmcc",              "中国移动"},
        {"china broadcast",   "中国广电"},
        {"chinabroadnet",     "中国广电"},
        {"cbn",               "中国广电"},
        {"great wall broadband", "长城宽带"},
        {"great wall",        "长城宽带"},
        {"dr.peng",           "鹏博士"},
        {"dr peng",           "鹏博士"},
        {"cernet",            "教育网"},
        {"education network", "教育网"},
        {"cstnet",            "中国科技网"},
        {"cnix",              "中国国际通信网"},
        {"cloudflare",        "Cloudflare"},
        {"amazon",            "Amazon AWS"},
        {"google",            "Google"},
        {"microsoft",         "Microsoft Azure"},
        {"akamai",            "Akamai"},
        {"hurricane electric","Hurricane Electric"},
    };
    for(const auto &r : rules)
    {
        if(lower.find(r.kw) != std::string::npos)
            return r.zh;
    }
    return raw;
}

// 异步查询本机出口公网 IP 的国家/省/市/运营商，直接调 ip-api.com 中文接口，
// 与 desktop/src-tauri 保持一致，所有字段中文化。失败时静默返回(footer 自然
// 跳过对应行)。仅供 PNG footer 展示，不影响测试主流程。
//
// 关键:webGet 第二参数传 "direct://" 显式禁用代理。语义跟 desktop 端
// get_my_ip_info() 的 .no_proxy() 一致。后端进程被 Tauri 启动时继承了
// HTTPS_PROXY/HTTP_PROXY env(用于自更新走系统代理),libcurl 默认读 env 会
// 把这查询也转给代理 → ip-api.com 看到的源 IP 是代理出口 → "测试机"显示
// 成代理节点位置(如美国 加州 洛杉矶 DMIT)。直连才能拿到本机真实出口。
static void fetchLocalGeoAsync()
{
    std::thread([](){
        std::string body = webGet(
            "http://ip-api.com/json/?lang=zh-CN&fields=country,regionName,city,isp,status",
            "direct://", 0);
        if(body.empty()) return;
        rapidjson::Document j;
        j.Parse(body.data());
        if(j.HasParseError()) return;
        if(GetMember(j, "status") != "success") return;
        g_local_country = GetMember(j, "country");
        g_local_region  = GetMember(j, "regionName");
        g_local_city    = GetMember(j, "city");
        g_local_isp     = translateIsp(GetMember(j, "isp"));
    }).detach();
}

// 形如 "(UTC+08:00)" 的时区标签，取自当前 tm 的 tm_gmtoff(Windows 用 _timezone)。
// strftime("%Z") 在 Windows 上会返回本地化全名("中国标准时间")，长度不可控且
// 容易撑破 footer，所以这里手工拼接。
static std::string buildTzLabel()
{
    long offset_sec = 0;
#ifdef _WIN32
    _tzset();
    offset_sec = -_timezone; // _timezone = UTC - local
    time_t now = time(NULL);
    struct tm *lt = localtime(&now);
    if(lt && lt->tm_isdst > 0) offset_sec += 3600;
#else
    time_t now = time(NULL);
    struct tm lt;
    localtime_r(&now, &lt);
    offset_sec = lt.tm_gmtoff;
#endif
    char sign = offset_sec >= 0 ? '+' : '-';
    long abs_sec = offset_sec >= 0 ? offset_sec : -offset_sec;
    int hh = static_cast<int>(abs_sec / 3600);
    int mm = static_cast<int>((abs_sec % 3600) / 60);
    char buf[16];
    snprintf(buf, sizeof(buf), "(UTC%c%02d:%02d)", sign, hh, mm);
    return std::string(buf);
}

// Returns true if any node in `nodes` has actual test data we'd want to save.
static bool hasAnyTestData(const std::vector<nodeInfo> &nodes)
{
    for(const auto &n : nodes)
    {
        if(n.totalRecvBytes > 0 || n.avgPing != "0.00")
            return true;
    }
    return false;
}

void signalHandler(int signum)
{
    std::cerr << "\n收到中断信号 (" << signum << ")，正在保存已完成的部分结果...\n";

#ifdef __APPLE__
    killClient(0);
#endif // __APPLE__
    killByHandle();

    // Best-effort: persist whatever was already tested so the user doesn't lose work
    // when they hit Ctrl+C mid-run. Any errors here are swallowed to avoid masking
    // the original interrupt.
    try
    {
        if(!allNodes.empty() && hasAnyTestData(allNodes))
        {
            // If batchTest already finished and called resultInit(), it picks
            // a "results/<time>.log" path. Switching that to "<time>-partial.log"
            // keeps interrupted runs visually distinct from completed ones.
            if(resultPath.empty())
                resultPath = "results" PATH_SLASH + getTime(1) + "-partial.log";
            else if(resultPath.find("-partial.log") == std::string::npos)
                resultPath = replace_all_distinct(resultPath, ".log", "-partial.log");

            // Run the same flag-emoji normalization the normal exit path does
            // (idempotent), so partial-save logs use the same [HK]-style
            // country tags as fully-completed runs.
            decorateNodeLabels(allNodes);

            saveResult(allNodes);
            writeLog(LOG_TYPE_INFO, "Partial result log written to " + resultPath);
            std::cerr << "已保存部分结果日志：" << resultPath << "\n";

            std::string pngpath = exportRender(resultPath, allNodes,
                                               export_with_maxspeed,
                                               export_sort_method,
                                               export_as_new_style,
                                               test_nat_type);
            writeLog(LOG_TYPE_INFO, "Partial result picture saved to " + pngpath);
            std::cerr << "已保存部分结果图片：" << pngpath << "\n";
        }
        else
        {
            std::cerr << "尚无完成的节点，无内容可保存。\n";
        }
    }
    catch(const std::exception &e)
    {
        writeLog(LOG_TYPE_ERROR, std::string("Failed to save partial result: ") + e.what());
    }
    catch(...)
    {
        writeLog(LOG_TYPE_ERROR, "Failed to save partial result: unknown error");
    }

    writeLog(LOG_TYPE_INFO, "Received signal. Exit right now.");
    logEOF();

    exit(signum);
}

void chkArg(int argc, char* argv[])
{
    for(int i = 0; i < argc; i++)
    {
        if(!strcmp(argv[i], "/rpc"))
            rpcmode = true;
        else if(!strcmp(argv[i], "/u") && argc > i + 1)
            sub_url.assign(argv[++i]);
        else if(!strcmp(argv[i], "/g") && argc > i + 1)
            custom_group.assign(argv[++i]);
#ifdef BUILD_WEBSERVER_ENGINE
        else if(!strcmp(argv[i], "/web"))
            webserver_mode = true;
#endif
    }
}

void saveResult(std::vector<nodeInfo> &nodes)
{
    INIReader ini;
    std::string data;

    ini.SetCurrentSection("Basic");
    ini.Set("Tester", "Stair Speedtest Reborn " VERSION);
    ini.Set("GenerationTime", getTime(3));

    for(nodeInfo &x : nodes)
    {
        ini.SetCurrentSection(x.group + "^" + x.remarks);
        ini.Set("AvgPing", x.avgPing);
        ini.Set("PkLoss", x.pkLoss);
        ini.Set("SitePing", x.sitePing);
        ini.Set("AvgSpeed", x.avgSpeed);
        ini.Set("MaxSpeed", x.maxSpeed);
        ini.Set("ULSpeed", x.ulSpeed);
        ini.SetNumber<unsigned long long>("UsedTraffic", x.totalRecvBytes);
        ini.SetNumber<int>("GroupID", x.groupID);
        ini.SetNumber<int>("ID", x.id);
        ini.SetBool("Online", x.online);
        ini.SetArray("RawPing", ",", x.rawPing);
        ini.SetArray("RawSitePing", ",", x.rawSitePing);
        ini.SetArray("RawSpeed", ",", x.rawSpeed);
    }

    ini.ToFile(resultPath);
}

std::string removeEmoji(const std::string &orig_remark)
{
    char emoji_id[2] = {(char)-16, (char)-97};
    std::string remark = orig_remark;
    while(true)
    {
        if(remark[0] == emoji_id[0] && remark[1] == emoji_id[1])
            remark.erase(0, 4);
        else
            break;
    }
    if(remark.empty())
        return orig_remark;
    return remark;
}

// Normalize node remarks for the result table.
//
// The SSRSpeed-style renderer now draws flag emoji NATIVELY (NotoColorEmoji via
// FreeType + HarfBuzz), so we must KEEP the raw 🇭🇰-style bytes in remarks
// instead of converting them to "[HK]" text. This pass therefore only trims
// surrounding whitespace; it is kept (with its call sites) so the signal
// handler / batchTest exit path still has one place to finalize labels.
static void decorateNodeLabels(std::vector<nodeInfo> &nodes)
{
    for(auto &n : nodes)
        n.remarks = trim(n.remarks);
}

int singleTest(nodeInfo &node)
{
    // Keep the raw flag emoji (🇭🇰 …) intact — the renderer draws it natively.
    node.remarks = trim(node.remarks);
    std::string logdata, testserver, username, password, proxy;
    int testport;
    node.ulTarget = def_upload_target; //for now only use default
    cur_node_id = node.id;
    std::string id = std::to_string(node.id + (rpcmode ? 0 : 1));

#ifdef BUILD_WEBSERVER_ENGINE
    // 入口快速退出:用户已经按了停止 → 不为这个节点启动任何探测
    // (延迟/GeoIP/NAT/下载),直接走完函数尾部由 batchTest 跳出循环。
    // singleTest 内部各个阻塞点(perform_test 累积循环)也单独检查 stop_requested,
    // 这里只是把"已经停了还要新开节点测试"的浪费提前到第一行就消除。
    if(stop_requested.load())
    {
        writeLog(LOG_TYPE_INFO, "Stop requested before node start, skipping: " + node.remarks);
        return SPEEDTEST_ERROR_NONE;
    }
#endif

    writeLog(LOG_TYPE_INFO, "Received server. Group: " + node.group + " Name: " + node.remarks);
    defer(printMsg(SPEEDTEST_MESSAGE_GOTRESULT, rpcmode, node.avgSpeed, node.maxSpeed, node.ulSpeed, node.pkLoss, node.avgPing, node.sitePing, node.natType.get());)
    auto start = steady_clock::now();
    if(node.proxyStr == "LOG") //import from result
    {
        if(!rpcmode)
            printMsg(SPEEDTEST_MESSAGE_GOTSERVER, rpcmode, id, node.group, replaceFlagEmojis(node.remarks), std::to_string(node_count));
        printMsg(SPEEDTEST_MESSAGE_GOTPING, rpcmode, id, node.avgPing);
        printMsg(SPEEDTEST_MESSAGE_GOTGPING, rpcmode, id, node.sitePing);
        printMsg(SPEEDTEST_MESSAGE_GOTSPEED, rpcmode, id, node.avgSpeed);
        printMsg(SPEEDTEST_MESSAGE_GOTUPD, rpcmode, id, node.ulSpeed);
        writeLog(LOG_TYPE_INFO, "Average speed: " + node.avgSpeed + "  Max speed: " + node.maxSpeed + "  Upload speed: " + node.ulSpeed + "  Traffic used in bytes: " + std::to_string(node.totalRecvBytes));
        return SPEEDTEST_ERROR_NONE;
    }
    defer(auto end = steady_clock::now(); auto lapse = duration_cast<seconds>(end - start); node.duration = lapse.count();)

    // 所有节点统一走 mihomo 内核:proxyStr 是内核里的节点名(切 outbound 用),
    // 测速/延迟都经本地 socks5。SOCKS5/HTTP 也由内核承载,不再有直连分支。
    testserver = socksaddr;
    testport = socksport;
    if(!node.proxyStr.empty() && node.proxyStr != "LOG")
    {
        // 健康守护:本节点开测前先确认 mihomo 还活着,死了就用既有 config 重启。
        // 不依赖 mihomoSwitchProxy 的失败重试 — 一旦内核进程退出,SwitchProxy
        // / SOCKS5 / /delay 全数 connect refused,只重试 SwitchProxy 救不回来。
        mihomoEnsureAlive();
        writeLog(LOG_TYPE_INFO, "Switching mihomo outbound to '" + node.proxyStr + "'...");
        if(!mihomoSwitchProxy(node.proxyStr))
        {
            // 失败立刻重试一次 — 端口刚才确认是活的,失败原因常见是 mihomo 在
            // 加载 provider 的瞬间把 GLOBAL selector 暂时锁住,200ms 后再 PUT
            // 通常就成功。重试还失败才算真死,直接放弃本节点 — 否则 outbound
            // 还停在上一节点上,本节点测的会是上一节点的速度,污染结果表。
            sleep(200);
            if(!mihomoSwitchProxy(node.proxyStr))
            {
                writeLog(LOG_TYPE_ERROR, "Proxy switch failed twice for '" + node.proxyStr + "', skipping this node to avoid measuring stale outbound.");
                node.avgPing = "0.00";
                node.sitePing = "0.00";
                node.pkLoss = "100.00%";
                node.avgSpeed = "N/A";
                node.maxSpeed = "N/A";
                node.ulSpeed = "N/A";
                node.online = false;
                return SPEEDTEST_ERROR_NOSPEED;
            }
        }
        // mihomo 切完 outbound 之后，QUIC 类协议(hy2/anytls/tuic)需要更长
        // 时间完成新链路握手 — TCP 协议 200ms 够，UDP 类至少 800ms 才稳。
        // 给所有协议统一 800ms 是最简单且不会拖慢太多的方案。
        sleep(800);
    }
    // Plan B: kernel is shared across the whole run, so per-test kill is gone.
    // (macOS still wipes leftover processes on signal exit.)
    proxy = buildSocks5ProxyString(testserver, testport, username, password);

    if(!rpcmode)
        printMsg(SPEEDTEST_MESSAGE_GOTSERVER, rpcmode, id, node.group, replaceFlagEmojis(node.remarks), std::to_string(node_count));
    // sleep moved above to right after the proxy switch; no extra wait needed here.
    writeLog(LOG_TYPE_INFO, "Now started fetching GeoIP info...");
    printMsg(SPEEDTEST_MESSAGE_STARTGEOIP, rpcmode, id);
    // inboundGeoIP:查节点服务器的 IP 元信息(归属地/ISP),应该用本机直连
    // api.ip.sb,绝不能走系统代理。Tauri sidecar 进程继承 HTTPS_PROXY env 后
    // libcurl 默认会读取并转发,导致这次查询绕了一圈代理出口。
    // outboundGeoIP:查"经过节点出口后看到的 IP",必须走该节点的 SOCKS5,
    // 这才是节点真实出口归属地。proxy 参数已是 socks5://127.0.0.1:port。
    node.inboundGeoIP.set(std::async(std::launch::async, [node](){ return getGeoIPInfo(node.server, "direct://"); }));
    node.outboundGeoIP.set(std::async(std::launch::async, [proxy](){ return getGeoIPInfo("", proxy); }));
    if(test_nat_type)
    {
        printMsg(SPEEDTEST_MESSAGE_STARTNAT, rpcmode, id);
        node.natType.set(std::async(std::launch::async, [testserver, testport, username, password](){ return get_nat_type_thru_socks5(testserver, testport, username, password); }));
    }

    printMsg(SPEEDTEST_MESSAGE_STARTPING, rpcmode, id);
    // 延迟测量:所有协议统一调用 mihomo `/proxies/<name>/delay` 接口,与
    // Clash Verge Rev / Karing 等所有 mihomo 客户端一致。
    //
    // 历史背景(为什么之前不一致 → 用户反馈"同 IP 不同协议延迟相差 5-6 倍"):
    //   * 旧版对 TCP 协议(VLESS / Trojan / SS / SSR / VMess)用 libcurl 经
    //     SOCKS5 + warmup + keep-alive 复用,只测纯 RTT(50ms)
    //   * 旧版对 QUIC 协议(Hysteria2 / Hysteria / TUIC / AnyTLS)用 mihomo
    //     /delay,每次包含完整握手(200-300ms)
    //   * 同节点同 IP,只换协议,数字差 5-6 倍,完全不可比
    //
    // 经查 mihomo 源码 (adapter/adapter.go::URLTest):
    //     start := time.Now()
    //     instance, err := p.DialContext(ctx, &addr)   // 每次新建 outbound
    //     defer instance.Close()                        // 测完立刻关
    //     resp, err := client.Do(req)                   // 一次 HEAD
    //     t = uint16(time.Since(start) / time.Millisecond)
    //   /delay 内部每次调用都是 "新建拨号 + HEAD 请求",没有连接池复用。
    //   每次都包含完整握手,所以连续多次采样得到的是同一拨号代价的多次抽样。
    //
    // 多次采样取均值:与 SpeedTest++ / NetMonster 等老牌测速工具的"延迟"列对齐。
    // 单次会被一次握手抖动放大成 +200ms,多次取均值能压住偶发抖动,数字稳定且
    // 仍跟 Clash Verge / Karing 单次值在同一量级(因为均值就是单次值的期望)。
    const std::string https_probe_url = "https://cp.cloudflare.com/generate_204";
    static const int LATENCY_PROBE_COUNT = 5;

    if(speedtest_mode != "speedonly")
    {
        double latency_ms = -1.0;
        int rawms[10] = {};
        int success_count = 0;
        long long sum_ms = 0;

        if(node.proxyStr.empty())
        {
            writeLog(LOG_TYPE_WARN, "Skipping latency probe: empty proxyStr (LOG-imported node?).");
        }
        else
        {
            writeLog(LOG_TYPE_INFO, "Measuring latency via mihomo /delay API (" + std::to_string(LATENCY_PROBE_COUNT) + " probes)...");
            for(int i = 0; i < LATENCY_PROBE_COUNT; ++i)
            {
                int ms = 0;
                if(mihomoMeasureDelay(node.proxyStr, &ms, https_probe_url))
                {
                    rawms[i] = ms;
                    sum_ms += ms;
                    ++success_count;
                }
                // 失败那次留 rawms[i]=0,前端历史抽样可见;均值只算成功项。
            }

            if(success_count > 0)
            {
                latency_ms = static_cast<double>(sum_ms) / success_count;
                writeLog(LOG_TYPE_INFO, "mihomo /delay: " + std::to_string(success_count) + "/"
                         + std::to_string(LATENCY_PROBE_COUNT) + " ok, avg "
                         + std::to_string(static_cast<int>(latency_ms)) + " ms");
            }
            else
            {
                // hy2 / anytls 等 QUIC 节点对 mihomo 内置 HEAD 不友好(已在历史
                // 测试 log 中观察到延迟 0 但下载正常),全部 mihomo 探测都失败时
                // 改走 socks5 自己测一次兜底。
                writeLog(LOG_TYPE_WARN, "mihomo /delay all failed for '" + node.proxyStr + "', trying socks5 HEAD fallback...");
                int fb_ms = 0;
                if(socks5MeasureDelay(testserver, testport, &fb_ms, https_probe_url))
                {
                    rawms[0] = fb_ms;
                    latency_ms = static_cast<double>(fb_ms);
                    writeLog(LOG_TYPE_INFO, "socks5 HEAD fallback succeeded: " + std::to_string(fb_ms) + " ms");
                }
                else
                {
                    writeLog(LOG_TYPE_WARN, "socks5 HEAD fallback also failed.");
                }
            }
        }

        std::move(std::begin(rawms), std::end(rawms), node.rawSitePing);
        for(int i = 0; i < 6 && i < 10; ++i) node.rawPing[i] = rawms[i];
        if(latency_ms < 0.0)
        {
            node.avgPing = "0.00";
            node.sitePing = "0.00";
            writeLog(LOG_TYPE_INFO, "Latency: all probes failed.");
        }
        else
        {
            char t[16] = {};
            snprintf(t, sizeof(t), "%0.2f", latency_ms);
            node.avgPing.assign(t);
            node.sitePing.assign(t);
            writeLog(LOG_TYPE_INFO, "Latency: " + node.sitePing + " ms");
        }
    }
    // pkLoss 在下载阶段后由实际下载结果决定:0 字节 → 100%,有数据 → 0%。
    // 延迟探测失败不影响 pkLoss(用户反馈"延迟测不出但下载正常"的场景需保留 online)。
    node.pkLoss = "0.00%";
    printMsg(SPEEDTEST_MESSAGE_GOTPING, rpcmode, id, node.avgPing, node.pkLoss);

    getTestFile(node, proxy, downloadFiles, matchRules, def_test_file);
    {
        geoIPInfo outbound = node.outboundGeoIP.get();
        if(outbound.organization.size())
        {
            writeLog(LOG_TYPE_INFO, "Got outbound ISP: " + outbound.organization + "  Country code: " + outbound.country_code);
            printMsg(SPEEDTEST_MESSAGE_GOTGEOIP, rpcmode, id, outbound.organization, outbound.country_code);
        }
        else
            printMsg(SPEEDTEST_ERROR_GEOIPERR, rpcmode, id);
        if(test_nat_type)
            printMsg(SPEEDTEST_MESSAGE_GOTNAT, rpcmode, id, node.natType.get());
    }

    // sitePing 与 avgPing 已同源，这里只补一次 GotGPing 消息，前端 webgui 仍能拿到
    if(test_site_ping)
        printMsg(SPEEDTEST_MESSAGE_GOTGPING, rpcmode, id, node.sitePing);

    printMsg(SPEEDTEST_MESSAGE_STARTSPEED, rpcmode, id);
    //node.total_recv_bytes = 1;
    if(speedtest_mode != "pingonly")
    {
        writeLog(LOG_TYPE_INFO, "Now performing file download speed test...");
        perform_test(node, testserver, testport, username, password, def_thread_count);
        logdata = std::accumulate(std::next(std::begin(node.rawSpeed)), std::end(node.rawSpeed), std::to_string(node.rawSpeed[0]), [](std::string a, int b){return std::move(a) + " " + std::to_string(b);});
        writeLog(LOG_TYPE_RAW, logdata);
        if(node.totalRecvBytes == 0)
        {
            writeLog(LOG_TYPE_ERROR, "Speedtest returned no speed.");
            printMsg(SPEEDTEST_ERROR_RETEST, rpcmode, id);
            perform_test(node, testserver, testport, username, password, def_thread_count);
            logdata = std::accumulate(std::next(std::begin(node.rawSpeed)), std::end(node.rawSpeed), std::to_string(node.rawSpeed[0]), [](std::string a, int b){return std::move(a) + " " + std::to_string(b);});
            writeLog(LOG_TYPE_RAW, logdata);
            if(node.totalRecvBytes == 0)
            {
                writeLog(LOG_TYPE_ERROR, "Speedtest returned no speed 2 times.");
                printMsg(SPEEDTEST_ERROR_NOSPEED, rpcmode, id);
                printMsg(SPEEDTEST_MESSAGE_GOTSPEED, rpcmode, id, node.avgSpeed, node.maxSpeed);
                return SPEEDTEST_ERROR_NOSPEED;
            }
        }
    }
    printMsg(SPEEDTEST_MESSAGE_GOTSPEED, rpcmode, id, node.avgSpeed, node.maxSpeed);
    if(test_upload)
    {
        writeLog(LOG_TYPE_INFO, "Now performing upload speed test...");
        printMsg(SPEEDTEST_MESSAGE_STARTUPD, rpcmode, id);
        upload_test(node, testserver, testport, username, password);
        printMsg(SPEEDTEST_MESSAGE_GOTUPD, rpcmode, id, node.ulSpeed);
    }
    writeLog(LOG_TYPE_INFO, "Average speed: " + node.avgSpeed + "  Max speed: " + node.maxSpeed + "  Upload speed: " + node.ulSpeed + "  Traffic used in bytes: " + std::to_string(node.totalRecvBytes));
    node.online = true;
    sleep(300);
    return SPEEDTEST_ERROR_NONE;
}

void batchTest(std::vector<nodeInfo> &nodes)
{
    nodeInfo node;
    unsigned int onlines = 0;
    long long tottraffic = 0;
    cur_node_id = -1;

    writeLog(LOG_TYPE_INFO, "Total node(s) found: " + std::to_string(node_count));
    if(node_count == 0)
    {
        writeLog(LOG_TYPE_ERROR, "No nodes are found in this subscription.");
        printMsg(SPEEDTEST_ERROR_NONODES, rpcmode);
    }
    else
    {
        // 一开测就记录时间与时区，renderer footer 用它代替"生成时间"。
        // custom_group 为空时使用第一个节点的 group(通常是协议默认 group)。
        g_test_start_time = getTime(3);
        g_test_tz_label = buildTzLabel();
        std::string log_group = custom_group;
        if(log_group.empty() && !nodes.empty())
            log_group = nodes.front().group;
        resultInit(log_group);
        // 异步查询本机出口位置 + 运营商，不阻塞测试主流程
        fetchLocalGeoAsync();

        writeLog(LOG_TYPE_INFO, "Speedtest will now begin.");
        printMsg(SPEEDTEST_MESSAGE_BEGIN, rpcmode);
        //first print out all nodes when in Web mode
        if(rpcmode)
        {
            for(nodeInfo &x : nodes)
                printMsg(SPEEDTEST_MESSAGE_GOTSERVER, rpcmode, std::to_string(x.id), x.group, x.remarks);
        }
        //then we start testing nodes
        for(auto &x : nodes)
        {
#ifdef BUILD_WEBSERVER_ENGINE
            // 前端 POST /stop 触发 → 当前节点跑完前不再启动下一个,直接跳出循环。
            // 已测节点的结果(写入 nodes 里的字段)保留,后续 saveResult / 渲染照常。
            if(stop_requested)
            {
                writeLog(LOG_TYPE_INFO, "Stop requested, breaking out of batch loop.");
                break;
            }
#endif
            if(custom_group.size() != 0)
                x.group = custom_group;
            // per-node 异常隔离:单节点异常(rapidjson assert / mihomo JSON 异常 /
            // socks5 状态机异常等)以前会冲出 batchTest 让 webgui 顶层 catch 兜住,
            // 后续节点全部被跳过 + saveResult/cur_node_id=-1 不再执行,前端拉到的
            // 是"半套结果 + status=stopped",体感"测了 20 / 48 就显示已完成"。
            // 这里把单节点失败收敛在节点内,整批继续跑完。
            try
            {
                singleTest(x);
            }
            catch(const std::exception &e)
            {
                writeLog(LOG_TYPE_ERROR, std::string("Node test exception: ") + x.remarks + " — " + e.what());
            }
            catch(...)
            {
                writeLog(LOG_TYPE_ERROR, "Node test exception (unknown): " + x.remarks);
            }
            tottraffic += x.totalRecvBytes;
            if(x.online)
                onlines++;
        }
        writeLog(LOG_TYPE_INFO, "All nodes tested. Total/Online nodes: " + std::to_string(node_count) + "/" + std::to_string(onlines) + " Traffic used: " + speedCalc(tottraffic * 1.0));
        decorateNodeLabels(nodes); // normalize flag emojis (e.g. 🇭🇰 -> [HK]) before persistence
        saveResult(nodes);
        if(!multilink)
        {
            printMsg(SPEEDTEST_MESSAGE_PICSAVING, rpcmode);
            writeLog(LOG_TYPE_INFO, "Now exporting result...");
            pngpath = exportRender(resultPath, nodes, export_with_maxspeed, export_sort_method, export_as_new_style, test_nat_type);
            writeLog(LOG_TYPE_INFO, "Result saved to " + pngpath + " .");
            printMsg(SPEEDTEST_MESSAGE_PICSAVED, rpcmode, pngpath);
            if(rpcmode)
                printMsg(SPEEDTEST_MESSAGE_PICDATA, rpcmode, "data:image/png;base64," + fileToBase64(pngpath));
        }
    }
    cur_node_id = -1;
}

void rewriteNodeID(std::vector<nodeInfo> &nodes)
{
    int index = 0;
    for(auto &x : nodes)
    {
        if(x.proxyStr == "LOG")
            return;
        x.id = index;
        index++;
    }
}

void rewriteNodeGroupID(std::vector<nodeInfo> &nodes, int groupID)
{
    std::for_each(nodes.begin(), nodes.end(), [&](nodeInfo &x){ x.groupID = groupID; });
}

void addNodes(std::string link, bool multilink)
{
    int linkType = -1;
    std::vector<nodeInfo> nodes;
    std::string strSub, strInput, fileContent, strProxy;

    link = replace_all_distinct(link, "\"", "");
    writeLog(LOG_TYPE_INFO, "Received Link.");

    // 来源四类:订阅 URL / data:upload / 本地文件 / 直接内容(单条分享链接或
    // 粘贴的订阅文本)。协议识别全部交给 mihomo,这里不再按 scheme 分发。
    if(startsWith(link, "http://") || startsWith(link, "https://") || startsWith(link, "surge:///install-config"))
        linkType = SPEEDTEST_MESSAGE_FOUNDSUB;
    else if(link == "data:upload")
        linkType = SPEEDTEST_MESSAGE_FOUNDUPD;
    else if(fileExist(link))
        linkType = SPEEDTEST_MESSAGE_FOUNDLOCAL;
    else
        linkType = SPEEDTEST_MESSAGE_MULTILINK; // 直接内容(交给 loadSubscription)

    // filterNodes 后统一套用用户自定义分组名(loadSubscription 不设 group)。
    auto applyGroup = [&](std::vector<nodeInfo> &ns){
        if(!custom_group.empty())
            for(auto &n : ns) n.group = custom_group;
    };

    switch(linkType)
    {
    case SPEEDTEST_MESSAGE_FOUNDSUB:
        printMsg(SPEEDTEST_MESSAGE_FOUNDSUB, rpcmode);
        if(!rpcmode && !multilink && !sub_url.size())
        {
            printMsg(SPEEDTEST_MESSAGE_GROUP, rpcmode);
            getline(std::cin, strInput);
            if(strInput.size())
            {
                custom_group = rpcmode ? strInput : ACPToUTF8(strInput);
                writeLog(LOG_TYPE_INFO, "Received custom group: " + custom_group);
            }
        }
        writeLog(LOG_TYPE_INFO, "Downloading subscription data...");
        printMsg(SPEEDTEST_MESSAGE_FETCHSUB, rpcmode);
        if(strFind(link, "surge:///install-config")) //surge config link
            link = UrlDecode(getUrlArg(link, "url"));
        // 第一次显式 direct:// 直连。Tauri 启动时给 sidecar 注入了 HTTPS_PROXY env
        // (供"检查更新"走代理),如果这里默认空 proxy,libcurl 会读 env 把订阅请求
        // 也转给代理,违反"测试相关请求只走本地网络"的约定。墙外订阅(GitHub raw /
        // 机场域名走 Cloudflare 等)直连失败时,下面 strProxy 兜底再走系统代理一次。
        strSub = webGet(link, "direct://");
        if(strSub.size() == 0)
        {
            //try to get it again with system proxy
            writeLog(LOG_TYPE_WARN, "Cannot download subscription directly. Using system proxy.");
            strProxy = getSystemProxy();
            if(strProxy.size())
            {
                printMsg(SPEEDTEST_ERROR_SUBFETCHERR, rpcmode);
                strSub = webGet(link, strProxy);
            }
            else
                writeLog(LOG_TYPE_WARN, "No system proxy is set. Skipping.");
        }
        if(strSub.size())
        {
            writeLog(LOG_TYPE_INFO, "Parsing subscription data...");
            loadSubscription(strSub, override_conf_port, nodes);
            filterNodes(nodes, custom_exclude_remarks, custom_include_remarks, curGroupID);
            applyGroup(nodes);
            copyNodes(nodes, allNodes);
        }
        else
        {
            writeLog(LOG_TYPE_ERROR, "Cannot download subscription data.");
            printMsg(SPEEDTEST_ERROR_INVALIDSUB, rpcmode);
        }
        break;
    case SPEEDTEST_MESSAGE_FOUNDLOCAL:
        printMsg(SPEEDTEST_MESSAGE_FOUNDLOCAL, rpcmode);
        if(!rpcmode && !multilink && !sub_url.size())
        {
            printMsg(SPEEDTEST_MESSAGE_GROUP, rpcmode);
            getline(std::cin, strInput);
            if(strInput.size())
            {
                custom_group = rpcmode ? strInput : ACPToUTF8(strInput);
                writeLog(LOG_TYPE_INFO, "Received custom group: " + custom_group);
            }
        }
        writeLog(LOG_TYPE_INFO, "Parsing configuration file data...");
        printMsg(SPEEDTEST_MESSAGE_PARSING, rpcmode);
        // 先试历史结果导入(.log),非历史文件再交给订阅归一化处理。
        if(explodeLog(fileGet(link), nodes) == -1)
        {
            if(loadSubscription(fileGet(link), override_conf_port, nodes) == 0)
            {
                printMsg(SPEEDTEST_ERROR_UNRECOGFILE, rpcmode);
                writeLog(LOG_TYPE_ERROR, "Invalid configuration file!");
                break;
            }
        }
        filterNodes(nodes, custom_exclude_remarks, custom_include_remarks, curGroupID);
        applyGroup(nodes);
        copyNodes(nodes, allNodes);
        break;
    case SPEEDTEST_MESSAGE_FOUNDUPD:
        printMsg(SPEEDTEST_MESSAGE_FOUNDUPD, rpcmode);
        std::cin.clear();
        //now we should ready to receive a large amount of data from stdin
        getline(std::cin, fileContent);
        fileContent = base64_decode(fileContent.substr(fileContent.find(",") + 1));
        writeLog(LOG_TYPE_INFO, "Parsing configuration file data...");
        printMsg(SPEEDTEST_MESSAGE_PARSING, rpcmode);
        if(loadSubscription(fileContent, override_conf_port, nodes) == 0)
        {
            printMsg(SPEEDTEST_ERROR_UNRECOGFILE, rpcmode);
            writeLog(LOG_TYPE_ERROR, "Invalid configuration file!");
        }
        else
        {
            filterNodes(nodes, custom_exclude_remarks, custom_include_remarks, curGroupID);
            applyGroup(nodes);
            copyNodes(nodes, allNodes);
        }
        break;
    default:
        // 直接内容:单条分享链接,或粘贴的 Clash YAML / base64 订阅文本。
        printMsg(SPEEDTEST_MESSAGE_FETCHSUB, rpcmode);
        if(loadSubscription(link, override_conf_port, nodes) == 0)
        {
            writeLog(LOG_TYPE_ERROR, "No valid link found.");
            printMsg(SPEEDTEST_ERROR_NORECOGLINK, rpcmode);
        }
        else
        {
            filterNodes(nodes, custom_exclude_remarks, custom_include_remarks, curGroupID);
            applyGroup(nodes);
            copyNodes(nodes, allNodes);
        }
    }
}

void setcd(std::string &file)
{
    char filename[256] = {};
    std::string path;
#ifdef _WIN32
    char szTemp[1024] = {};
    char *pname = NULL;
    DWORD retVal = GetFullPathName(file.data(), 1023, szTemp, &pname);
    if(!retVal)
        return;
    strcpy(filename, pname);
    strrchr(szTemp, '\\')[1] = '\0';
    path.assign(szTemp);
#else
    char *ret = realpath(file.data(), NULL);
    if(ret == NULL)
        return;
    strncpy(filename, strrchr(ret, '/') + 1, 255);
    strrchr(ret, '/')[1] = '\0';
    path.assign(ret);
    free(ret);
#endif // _WIN32
    file.assign(filename);
    chdir(path.data());
}

int main(int argc, char* argv[])
{
    std::vector<nodeInfo> nodes;
    nodeInfo node;
    std::string link;
    std::string curPNGPath, curPNGPathPrefix;
    std::cout << std::fixed;
    std::cout << std::setprecision(2);

#ifndef _DEBUG
    std::string prgpath = argv[0];
    setcd(prgpath); //switch to program directory
#endif // _DEBUG
    chkArg(argc, argv);

    makeDir("logs");
    makeDir("results");
    logInit(rpcmode);
    readConf("pref.ini");
#ifdef _WIN32
    //start up windows socket library first
    WSADATA wsd;
    if(WSAStartup(MAKEWORD(2, 2), &wsd) != 0)
    {
        printMsg(SPEEDTEST_ERROR_WSAERR, rpcmode);
        return -1;
    }
    //along with some console window info
    SetConsoleOutputCP(65001);
#else
    signal(SIGCHLD, SIG_IGN);
    signal(SIGPIPE, SIG_IGN);
    signal(SIGABRT, SIG_IGN);
    signal(SIGHUP, signalHandler);
    signal(SIGQUIT, signalHandler);
#endif // _WIN32
    signal(SIGTERM, signalHandler);
    signal(SIGINT, signalHandler);

    if(!rpcmode)
        SetConsoleTitle("Stair Speedtest Reborn " VERSION);

    //kill any leftover mihomo before testing (only macOS keeps the kernel
    //alive across runs; on Windows JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE handles it).
#ifdef __APPLE__
    killClient(0);
#endif // __APPLE__
    clientCheck();
    initUserAgent(); // build "Clash mihomo/<ver> stairspeedtest-reborn" UA
    if(!rpcmode)
        checkMihomoUpdate(); // background check; prints a hint only if a newer kernel exists
    socksport = checkPort(socksport);
    writeLog(LOG_TYPE_INFO, "Using local port: " + std::to_string(socksport));
    writeLog(LOG_TYPE_INFO, "Init completed.");

#ifdef BUILD_WEBSERVER_ENGINE
    // 如果 pref.ini 或 /web 参数指定了 webserver 模式,把控制权交给 webgui_wrapper,
    // 启动 HTTP 服务后在此线程内阻塞处理请求(start_web_server_multi 阻塞返回前
    // 永远不退出),不再走下面的交互式 CLI 测速流程。
    if(webserver_mode)
    {
        ssrspeed_webserver_routine(listen_address, listen_port);
        return 0;
    }
#endif
    //intro message
    printMsg(SPEEDTEST_MESSAGE_WELCOME, rpcmode);
    if(sub_url.size())
    {
        link = sub_url;
        std::cout<<"已从命令行参数读取链接。\n"<<std::endl;
    }
    else
    {
        getline(std::cin, link);
        if(!rpcmode)
            link = ACPToUTF8(link);
        writeLog(LOG_TYPE_INFO, "Input data: " + link);
        if(rpcmode)
        {
            string_array webargs = split(link, "^");
            if(webargs.size() == 6)
            {
                link = webargs[0];
                if(webargs[1] != "?empty?")
                    custom_group = webargs[1];
                speedtest_mode = webargs[2];
                export_sort_method = webargs[4];
                export_with_maxspeed = webargs[5] == "true";
            }
            else
            {
                link = "?empty?";
            }
        }
    }

    if(strFind(link, "|"))
    {
        multilink = true;
        printMsg(SPEEDTEST_MESSAGE_MULTILINK, rpcmode);
        string_array linkList = split(link, "|");
        for(auto &x : linkList)
        {
            addNodes(x, multilink);
            curGroupID++;
        }
    }
    else
    {
        addNodes(link, multilink);
    }
    rewriteNodeID(allNodes); //reset all index
    node_count = allNodes.size();

    // ----- Plan B: start mihomo once with every node packed into one config -----
    bool kernel_started = launchMihomoForNodes(allNodes);
    // Ensure the kernel is killed when main returns naturally too. Idempotent.
    defer(if(kernel_started) { writeLog(LOG_TYPE_INFO, "Stopping mihomo at program exit."); killByHandle(); });

    if(allNodes.size() > 1) //group or multi-link
    {
        batchTest(allNodes);
        if(multilink)
        {
            if(multilink_export_as_one_image)
            {
                printMsg(SPEEDTEST_MESSAGE_PICSAVING, rpcmode);
                writeLog(LOG_TYPE_INFO, "Now exporting result...");
                curPNGPath = replace_all_distinct(resultPath, ".log", "") + "-multilink-all.png";
                pngpath = exportRender(curPNGPath, allNodes, export_with_maxspeed, export_sort_method, export_as_new_style, test_nat_type);
                printMsg(SPEEDTEST_MESSAGE_PICSAVED, rpcmode, pngpath);
                writeLog(LOG_TYPE_INFO, "Result saved to " + pngpath + " .");
                if(rpcmode)
                    printMsg(SPEEDTEST_MESSAGE_PICDATA, rpcmode, "data:image/png;base64," + fileToBase64(pngpath));
            }
            else
            {
                printMsg(SPEEDTEST_MESSAGE_PICSAVING, rpcmode);
                curPNGPathPrefix = replace_all_distinct(resultPath, ".log", "");
                for(int i = 0; i < curGroupID; i++)
                {
                    eraseElements(nodes);
                    copyNodesWithGroupID(allNodes, nodes, i);
                    if(!nodes.size())
                        break;
                    if((nodes.size() == 1 && single_test_force_export) || nodes.size() > 1)
                    {
                        printMsg(SPEEDTEST_MESSAGE_PICSAVINGMULTI, rpcmode, std::to_string(i + 1));
                        writeLog(LOG_TYPE_INFO, "Now exporting result for group " + std::to_string(i + 1) + "...");
                        curPNGPath = curPNGPathPrefix + "-multilink-group" + std::to_string(i + 1) + ".png";
                        pngpath = exportRender(curPNGPath, nodes, export_with_maxspeed, export_sort_method, export_as_new_style, test_nat_type);
                        printMsg(SPEEDTEST_MESSAGE_PICSAVEDMULTI, rpcmode, std::to_string(i + 1), pngpath);
                        writeLog(LOG_TYPE_INFO, "Group " + std::to_string(i + 1) + " result saved to " + pngpath + " .");
                    }
                    else
                        writeLog(LOG_TYPE_INFO, "Group " + std::to_string(i + 1) + " result export skipped.");
                }
            }
        }
        writeLog(LOG_TYPE_INFO, "Multi-link test completed.");
    }
    else if(allNodes.size() == 1)
    {
        // single-node path doesn't go through batchTest(), so we need to
        // initialize the result log path ourselves before testing — otherwise
        // the signal handler (Ctrl+C) and the post-test save below would have
        // nowhere to write.
        g_test_start_time = getTime(3);
        g_test_tz_label = buildTzLabel();
        std::string log_group = custom_group.empty() ? allNodes.front().group : custom_group;
        resultInit(log_group);
        fetchLocalGeoAsync();
        writeLog(LOG_TYPE_INFO, "Speedtest will now begin.");
        printMsg(SPEEDTEST_MESSAGE_BEGIN, rpcmode);
        singleTest(allNodes[0]);
        decorateNodeLabels(allNodes);
        saveResult(allNodes);
        printMsg(SPEEDTEST_MESSAGE_PICSAVING, rpcmode);
        writeLog(LOG_TYPE_INFO, "Now exporting result...");
        pngpath = exportRender(resultPath, allNodes, export_with_maxspeed, export_sort_method, export_as_new_style, test_nat_type);
        printMsg(SPEEDTEST_MESSAGE_PICSAVED, rpcmode, pngpath);
        writeLog(LOG_TYPE_INFO, "Result saved to " + pngpath + " .");
        if(rpcmode)
            printMsg(SPEEDTEST_MESSAGE_PICDATA, rpcmode, "data:image/png;base64," + fileToBase64(pngpath));
        writeLog(LOG_TYPE_INFO, "Single node test completed.");
    }
    else
    {
        writeLog(LOG_TYPE_ERROR, "No valid link found.");
        printMsg(SPEEDTEST_ERROR_NORECOGLINK, rpcmode);
    }
    logEOF();
    printMsg(SPEEDTEST_MESSAGE_EOF, rpcmode);
    sleep(1);
    //std::cin.clear();
    //std::cin.ignore();
    if(!rpcmode && sub_url.size() && pause_on_done)
        _getch();
#ifdef _WIN32
    //stop socket library before exit
    WSACleanup();
#else
    std::cout<<std::endl;
#endif // _WIN32
    return 0;
}
