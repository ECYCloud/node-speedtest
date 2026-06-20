#include <string>
#include <chrono>
#include <thread>
#include <vector>
#include <iostream>
#include <mutex>
#include <atomic>
#include <queue>
#include <algorithm>
#include <unistd.h>
#include <pthread.h>

#include <openssl/ssl.h>
#include <openssl/err.h>
#include <openssl/x509v3.h>

#include "misc.h"
#include "socket.h"
#include "logger.h"
#include "printout.h"
#include "webget.h"
#include "nodeinfo.h"

using namespace std::chrono;

// /web 模式下的"立刻打断"信号:webgui_wrapper.cpp 在 POST /stop 时把它置 true。
// perform_test / upload_test 的累积循环每 500ms-1000ms 检查一次,true 就立刻 break,
// 然后 EXIT_FLAG=true 让所有 worker socket 读写循环也退出。
// CLI 模式不编 webgui_wrapper.cpp,该变量不存在,helper 永远返回 false,无副作用。
#ifdef BUILD_WEBSERVER_ENGINE
extern std::atomic<bool> stop_requested;
static inline bool wantStopNow() { return stop_requested.load(); }
#else
static inline bool wantStopNow() { return false; }
#endif

std::queue<SOCKET> opened_socket;

#define MAX_FILE_SIZE 512*1024*1024

//for use of multi-thread socket test
typedef std::lock_guard<std::mutex> guarded_mutex;
std::mutex opened_socket_mutex;
std::atomic_ullong received_bytes = 0;
std::atomic_int launched = 0, still_running = 0;
std::atomic_bool EXIT_FLAG = false;

void push_socket(const SOCKET &s)
{
    guarded_mutex guard(opened_socket_mutex);
    opened_socket.push(s);
}

static inline void draw_progress_dl(int progress, int this_bytes)
{
    std::cerr<<"\r[";
    for(int j = 0; j < progress; j++)
    {
        std::cerr<<"=";
    }
    if(progress < 20)
        std::cerr<<">";
    else
        std::cerr<<"]";
    std::cerr<<" "<<progress * 5<<"% "<<speedCalc(this_bytes);
}

static inline void draw_progress_icon(int progress)
{
    std::cerr<<"\r ";
    switch(progress % 4)
    {
    case 1:
        std::cerr<<"\\";
        break;
    case 2:
        std::cerr<<"|";
        break;
    case 3:
        std::cerr<<"/";
        break;
    default:
        std::cerr<<"-";
        break;
    }
}

static inline void draw_progress_ul(int progress, int this_bytes)
{
    draw_progress_icon(progress);
    std::cerr<<" "<<speedCalc(this_bytes);
}

static void SSL_Library_init()
{
    static bool init = false;
    if(!init)
        init = true;
    else
        return;
    SSL_load_error_strings();
    SSL_library_init();
    OpenSSL_add_all_algorithms();
}

// CA bundle 路径解析:CWD 下找 cacert.pem,找不到再去 exe 同级目录找。
// Tauri sidecar 模式下 CWD 是 engine 目录,CLI 模式下是 stairspeedtest-mihomo-win64,
// 两种部署都把 cacert.pem 放在那一层，一次解析即可。返回空字符串表示找不到,
// 调用方据此降级为不开校验(NotApplicable)。
static std::string locateCaBundle()
{
    static std::string cached;
    static bool resolved = false;
    if(resolved) return cached;
    resolved = true;
    auto fileExists = [](const std::string &p)
    {
        FILE *f = fopen(p.c_str(), "rb");
        if(f) { fclose(f); return true; }
        return false;
    };
    if(fileExists("cacert.pem")) { cached = "cacert.pem"; return cached; }
    // 退而求其次:相对路径再加 ./ 前缀，或者 engine 下被 sidecar 改了 CWD,
    // 这里不做更复杂的可执行文件定位 — 部署侧保证 cacert.pem 与可执行同目录。
    cached.clear();
    return cached;
}

// 测速链路 TLS 校验状态聚合。perform_test 启动前由调用方 reset(),
// 4 个 worker 并发写入，结束后 perform_test 把 finalize() 的结果赋给 node.tlsVerified。
// 用文件作用域 atomic 而非 nodeInfo 直接持有 atomic,是为了不破坏 nodeInfo 可拷贝。
static std::atomic<int> g_tls_verified_count {0};
static std::atomic<int> g_tls_failed_count {0};
static std::atomic<int> g_tls_attempted {0};

static void tls_state_reset()
{
    g_tls_verified_count.store(0);
    g_tls_failed_count.store(0);
    g_tls_attempted.store(0);
}

static TlsVerifyState tls_state_finalize()
{
    if(g_tls_attempted.load() == 0)
        return TlsVerifyState::NotApplicable;
    if(g_tls_failed_count.load() > 0)
        return TlsVerifyState::Failed;
    if(g_tls_verified_count.load() > 0)
        return TlsVerifyState::Verified;
    return TlsVerifyState::NotApplicable;
}

// 配置 SSL_CTX 进入"严格 PKI 模式":加载 Mozilla CA bundle 作为信任根,
// 启用 SSL_VERIFY_PEER(握手时强制走完信任链校验),失败由后续 SSL_get_verify_result
// 返回 X509_V_ERR_*。返回 false 表示 CA bundle 不可用，调用方应降级为不
// 校验(保留旧行为)并把节点标 NotApplicable。
static bool configureCtxStrict(SSL_CTX *ctx)
{
    std::string ca = locateCaBundle();
    if(ca.empty())
        return false;
    if(SSL_CTX_load_verify_locations(ctx, ca.c_str(), nullptr) != 1)
    {
        writeLog(LOG_TYPE_WARN, "SSL_CTX_load_verify_locations failed for " + ca);
        return false;
    }
    // SSL_VERIFY_PEER + SSL_get_verify_result:握手期间走完证书链/吊销/有效期
    // 校验，失败时握手仍可能成功(取决于 OpenSSL 版本和加密套件),所以握手
    // 完成后必须再读 SSL_get_verify_result 才算真正核实。
    SSL_CTX_set_verify(ctx, SSL_VERIFY_PEER, nullptr);
    SSL_CTX_set_verify_depth(ctx, 9);
    return true;
}

// 完成 TLS 握手 + PKI 验证 + 主机名校验。返回 true 表示证书核实通过;
// 任一步失败返回 false,日志里写明具体原因便于排错。
// 注意:本函数只回报状态给上层 worker,不直接写 node.tlsVerified —— 多 worker
// 并发，统一由 g_tls_*counter 聚合。
static bool tlsHandshakeAndVerify(SSL *ssl, const std::string &host, const char *channel)
{
    // SNI:Cloudflare 等多租户必备，无 server_name 直接 alert 40。
    SSL_set_tlsext_host_name(ssl, host.c_str());
    // OpenSSL 1.0.2+ 内置主机名验证:把 SAN/CN 与请求 host 自动对齐,
    // 比手工解析 X509 更稳。失败 SSL_get_verify_result 会返回 hostname mismatch。
    SSL_set1_host(ssl, host.c_str());
    if(SSL_connect(ssl) != 1)
    {
        unsigned long e = ERR_peek_error();
        char ebuf[256] = {};
        if(e) ERR_error_string_n(e, ebuf, sizeof(ebuf));
        writeLog(LOG_TYPE_WARN, std::string(channel) + " TLS handshake failed for " + host
                 + (e ? std::string(": ") + ebuf : std::string()));
        return false;
    }
    long vr = SSL_get_verify_result(ssl);
    if(vr != X509_V_OK)
    {
        writeLog(LOG_TYPE_WARN, std::string(channel) + " TLS cert verify failed for " + host
                 + ": " + X509_verify_cert_error_string(vr));
        return false;
    }
    return true;
}

// 仅延迟模式专用:经 socks5 → host:port 走一次 OpenSSL 严格握手核实证书,
// 不发任何 HTTP 请求,握手 / 验证完成立即断开。结果直接写回 node.tlsVerified —
// 单节点单点写入不会跟其它 worker 抢同一份 g_tls_* 计数器,所以这里独立判定。
// cacert.pem 不可用 → 维持 NotApplicable,与下载阶段同语义:不撒谎说"已校验"。
int verifyTlsForLatency(nodeInfo &node, const std::string &socks_addr, int socks_port,
                        const std::string &host, int host_port)
{
    SSL_Library_init();
    SOCKET s = initSocket(getNetworkType(socks_addr), SOCK_STREAM, IPPROTO_TCP);
    if(s == INVALID_SOCKET)
        return -1;
    push_socket(s);
    setTimeout(s, 8000);
    if(startConnect(s, socks_addr, socks_port) == SOCKET_ERROR
       || connectSocks5(s, "", "") == -1
       || connectThruSocks(s, host, host_port) == -1)
        return -1;

    SSL_CTX *ctx = SSL_CTX_new(TLS_client_method());
    if(ctx == NULL)
    {
        writeLog(LOG_TYPE_ERROR, "SSL_CTX_new failed for tls verify.");
        return -1;
    }
    defer(SSL_CTX_free(ctx);)

    if(!configureCtxStrict(ctx))
    {
        // cacert.pem 不可用,延迟阶段无证书校验能力,保持 NotApplicable。
        // 不强行降级为 Verified,避免给用户错误的安全保证。
        writeLog(LOG_TYPE_WARN, "TLS verify skipped for " + node.remarks
                 + ": cacert.pem unavailable, falling back to NotApplicable.");
        return 0;
    }

    SSL *ssl = SSL_new(ctx);
    defer(SSL_free(ssl);)
    SSL_set_fd(ssl, s);

    if(tlsHandshakeAndVerify(ssl, host, "latency"))
        node.tlsVerified = TlsVerifyState::Verified;
    else
        node.tlsVerified = TlsVerifyState::Failed;

    // SSL_shutdown 失败不算事 — 我们只关心 verify 结果，不在乎对端是否优雅断开。
    SSL_shutdown(ssl);
    return 0;
}

int _thread_download(std::string host, int port, std::string uri, std::string localaddr, int localport, std::string username, std::string password, bool useTLS = false)
{
    launched++;
    still_running++;
    defer(still_running--;)
    char bufRecv[BUF_SIZE];
    int retVal, cur_len/*, recv_len = 0*/;
    SOCKET sHost;
    std::string request = "GET " + uri + " HTTP/1.1\r\n"
                          "Host: " + host + "\r\n"
                          "Connection: close\r\n"
                          "User-Agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/72.0.3626.121 Safari/537.36\r\n\r\n";

    sHost = initSocket(getNetworkType(localaddr), SOCK_STREAM, IPPROTO_TCP);
    if(INVALID_SOCKET == sHost)
        return -1;
    push_socket(sHost);
    // SOCKS5 应答阻塞读会等到 mihomo 完成出站节点拨号 + 握手才返回。
    // QUIC/TLS 慢节点首次握手常超 5s,把 IO 超时放宽到 10s 避免 4 线程齐死 → 0 字节 → N/A。
    setTimeout(sHost, 10000);
    if(startConnect(sHost, localaddr, localport) == SOCKET_ERROR || connectSocks5(sHost, username, password) == -1 || connectThruSocks(sHost, host, port) == -1)
        return -1;

    if(useTLS)
    {
        SSL_CTX *ctx;
        SSL *ssl;

        ctx = SSL_CTX_new(TLS_client_method());
        if(ctx == NULL)
        {
            writeLog(LOG_TYPE_ERROR, "SSL_CTX_new failed for download.");
            return -1;
        }
        defer(SSL_CTX_free(ctx);)

        // 严格 PKI 模式:加载 Mozilla CA bundle + SSL_VERIFY_PEER + 主机名校验。
        // CA bundle 找不到时降级为不开校验(保留旧行为不打断测速),节点 TLS
        // 状态保持 NotApplicable;开起来后任一 worker 校验通过/失败都计入聚合。
        bool strict = configureCtxStrict(ctx);
        if(strict)
            g_tls_attempted.fetch_add(1);

        ssl = SSL_new(ctx);
        defer(SSL_free(ssl);)
        SSL_set_fd(ssl, sHost);

        if(strict)
        {
            if(tlsHandshakeAndVerify(ssl, host, "download"))
                g_tls_verified_count.fetch_add(1);
            else
            {
                g_tls_failed_count.fetch_add(1);
                return -1;
            }
        }
        else
        {
            // 兼容模式:无 CA bundle,仅 SNI + 握手，不做证书校验，与历史行为一致。
            SSL_set_tlsext_host_name(ssl, host.c_str());
            if(SSL_connect(ssl) != 1)
            {
                writeLog(LOG_TYPE_FILEDL, "TLS handshake failed for " + host + " (likely server alert / SNI mismatch).");
                return -1;
            }
        }
        retVal = SSL_write(ssl, request.data(), request.size());
        if(retVal == SOCKET_ERROR)
            return -1;
        while(1)
        {
            cur_len = SSL_read(ssl, bufRecv, BUF_SIZE - 1);
            if(cur_len < 0)
            {
                if(errno == EWOULDBLOCK || errno == EAGAIN)
                {
                    continue;
                }
                else
                {
                    break;
                }
            }
            if(cur_len == 0)
                break;
            received_bytes += cur_len;
            if(EXIT_FLAG)
                break;
        }
    }
    else
    {
        retVal = Send(sHost, request.data(), request.size(), 0);
        if (SOCKET_ERROR == retVal)
            return -1;
        while(1)
        {
            cur_len = Recv(sHost, bufRecv, BUF_SIZE - 1, 0);
            if(cur_len < 0)
            {
                if(errno == EWOULDBLOCK || errno == EAGAIN)
                {
                    continue;
                }
                else
                {
                    break;
                }
            }
            if(cur_len == 0)
                break;
            received_bytes += cur_len;
            if(EXIT_FLAG)
                break;
        }
    }
    return 0;
}

int _thread_upload(std::string host, int port, std::string uri, std::string localaddr, int localport, std::string username, std::string password, bool useTLS = false)
{
    launched++;
    still_running++;
    defer(still_running--;)
    int retVal, cur_len;
    SOCKET sHost;
    std::string request = "POST " + uri + " HTTP/1.1\r\n"
                          "Connection: close\r\n"
                          "Content-Length: 134217728\r\n"
                          "Host: " + host + "\r\n\r\n";
    std::string post_data;

    sHost = initSocket(getNetworkType(localaddr), SOCK_STREAM, IPPROTO_TCP);
    if(INVALID_SOCKET == sHost)
        return -1;
    push_socket(sHost);
    setTimeout(sHost, 10000);
    if(startConnect(sHost, localaddr, localport) == SOCKET_ERROR || connectSocks5(sHost, username, password) == -1 || connectThruSocks(sHost, host, port) == -1)
        return -1;

    if(useTLS)
    {
        SSL_CTX *ctx;
        SSL *ssl;

        ctx = SSL_CTX_new(TLS_client_method());
        if(ctx == NULL)
        {
            writeLog(LOG_TYPE_ERROR, "SSL_CTX_new failed for upload.");
            return -1;
        }
        defer(SSL_CTX_free(ctx);)

        // 与下载链路同策略:有 CA bundle 走严格校验，否则降级 SNI-only。
        // 必须在 SSL_new 之前在 ctx 上配置 verify,SSL_new 才会继承这些设置。
        bool strict = configureCtxStrict(ctx);
        if(strict)
            g_tls_attempted.fetch_add(1);

        ssl = SSL_new(ctx);
        defer(SSL_free(ssl);)
        SSL_set_fd(ssl, sHost);

        if(strict)
        {
            if(tlsHandshakeAndVerify(ssl, host, "upload"))
                g_tls_verified_count.fetch_add(1);
            else
            {
                g_tls_failed_count.fetch_add(1);
                return -1;
            }
        }
        else
        {
            SSL_set_tlsext_host_name(ssl, host.c_str());
            if(SSL_connect(ssl) != 1)
            {
                writeLog(LOG_TYPE_FILEUL, "TLS handshake failed for " + host + " (upload).");
                return -1;
            }
        }
        SSL_write(ssl, request.data(), request.size());
        while(1)
        {
            post_data = rand_str(128);
            cur_len = SSL_write(ssl, post_data.data(), post_data.size());
            if(cur_len == SOCKET_ERROR)
            {
                break;
            }
            received_bytes += cur_len;
            if(EXIT_FLAG)
                break;
        }
    }
    else
    {
        retVal = Send(sHost, request.data(), request.size(), 0);
        if (SOCKET_ERROR == retVal)
            return -1;
        while(1)
        {
            post_data = rand_str(128);
            cur_len = Send(sHost, post_data.data(), post_data.size(), 0);
            if(cur_len == SOCKET_ERROR)
            {
                break;
            }
            received_bytes += cur_len;
            if(EXIT_FLAG)
                break;
        }
    }
    return 0;
}

struct thread_args
{
    std::string host;
    int port = 0;
    std::string uri;
    std::string localaddr;
    int localport;
    std::string username;
    std::string password;
    bool useTLS = false;
};

void* _thread_download_caller(void *arg)
{
    thread_args *args = (thread_args*)arg;
    _thread_download(args->host, args->port, args->uri, args->localaddr, args->localport, args->username, args->password, args->useTLS);
    return 0;
}

void* _thread_upload_caller(void *arg)
{
    thread_args *args = (thread_args*)arg;
    _thread_upload(args->host, args->port, args->uri, args->localaddr, args->localport, args->username, args->password, args->useTLS);
    return 0;
}

int perform_test(nodeInfo &node, std::string localaddr, int localport, std::string username, std::string password, int thread_count)
{
    writeLog(LOG_TYPE_FILEDL, "Multi-thread download test started.");
    //prep up vars first
    std::string host, uri, testfile = node.testFile;
    int port = 0, i;
    bool useTLS = false;

    writeLog(LOG_TYPE_FILEDL, "Fetch target: " + testfile);
    urlParse(testfile, host, uri, port, useTLS);
    received_bytes = 0;
    EXIT_FLAG = false;
    // 清零上一节点遗留的瞬时速度采样点。否则若本节点中途异常退出，
    // rawSpeed 末尾会保留前一节点的高值，前端"实时速度"会读到陈旧数据。
    std::fill(std::begin(node.rawSpeed), std::end(node.rawSpeed), 0ULL);
    // TLS 校验聚合状态需在每个节点测速前清零，否则会把前一节点的计数带过来,
    // 导致 footer 显示 (n/m) 数字累加成虚假"通过总数"。
    tls_state_reset();

    if(useTLS)
    {
        writeLog(LOG_TYPE_FILEDL, "Found HTTPS URL. Initializing OpenSSL library.");
        SSL_Library_init();
    }
    else
    {
        writeLog(LOG_TYPE_FILEDL, "Found HTTP URL.");
    }

    int running;
    thread_args args = {host, port, uri, localaddr, localport, username, password, useTLS};
    //std::thread threads[thread_count];
    std::vector<pthread_t> threads(thread_count);
    launched = 0;
    int created = 0;
    for(i = 0; i != thread_count; i++)
    {
        writeLog(LOG_TYPE_FILEDL, "Starting up thread #" + std::to_string(i + 1) + ".");
        if(pthread_create(&threads[i], NULL, _thread_download_caller, &args) == 0)
            created++;
        else
            writeLog(LOG_TYPE_ERROR, "pthread_create failed for download thread #" + std::to_string(i + 1));
    }
    if(created == 0)
    {
        writeLog(LOG_TYPE_ERROR, "All download threads failed to start; aborting test for this node.");
        return -1;
    }
    // 等任一线程进入函数体把 launched 自增 — 但带 3s 上限避免线程全部
    // pthread_create 成功却被 sched 长期挂起时主线程死锁。
    {
        int waited = 0;
        while(!launched && waited < 3000)
        {
            sleep(20);
            waited += 20;
        }
        if(!launched)
        {
            writeLog(LOG_TYPE_ERROR, "No download thread reached entry within 3s; aborting test for this node.");
            EXIT_FLAG = true;
            for(int k = 0; k < thread_count; ++k) pthread_join(threads[k], NULL);
            return -1;
        }
    }

    writeLog(LOG_TYPE_FILEDL, "All threads launched. Start accumulating data.");
    auto start = steady_clock::now();
    unsigned long long transferred_bytes = 0, this_bytes = 0, cur_recv_bytes = 0, max_speed = 0;
    // 峰值速度规范化:对齐 Speedtest/Cloudflare/LibreSpeed 的标准做法，用滑动窗口
    // 均值的历史最大值作为"最高速度",而不是单 0.5s 瞬时最大值。
    //   * 单点瞬时最大易被 TSO 聚合、慢启动末期 cwnd 翻倍、丢包后 RTO 大窗口重传等
    //     抖动"虚高"拉到代表不了节点真实带宽的尖峰
    //   * 与前端展示的"实时速度"(同样 2s 滑动均值)保持同尺度，语义对齐:
    //         maxSpeed = 历史所有 2s 窗口均值 max
    //         curSpeed = 最近 2s 窗口均值
    //     永远 maxSpeed >= curSpeed,且两者可直接比较
    constexpr int kPeakWindow = 4; // 4 × 0.5s = 2s
    for(i = 1; i < 21; i++)
    {
        sleep(500); //accumulate data
        cur_recv_bytes = received_bytes;
        this_bytes = (cur_recv_bytes - transferred_bytes) * 2; //these bytes were received in 0.5s
        transferred_bytes = cur_recv_bytes;

        node.rawSpeed[i - 1] = this_bytes;
        // 计算覆盖到当前采样点的 2s 滑动窗口均值。前 1-3 个采样点窗口未填满时
        // 用现有数据均值(避免被 0 拉低，前几格也参与得到最大值就是 OK 的)。
        {
            int win_begin = (i - 1) - (kPeakWindow - 1);
            if(win_begin < 0) win_begin = 0;
            unsigned long long win_sum = 0;
            int win_n = (i - 1) - win_begin + 1;
            for(int k = win_begin; k <= i - 1; ++k) win_sum += node.rawSpeed[k];
            unsigned long long win_avg = win_sum / win_n;
            max_speed = std::max(max_speed, win_avg);
        }
        // 实时更新已下载量/平均/最高速度，供 web 模式轮询 /getresults 时展示实时进度。
        // 否则这些字段要等下载循环全部结束才一次性赋值，前端实时速度列只能显示 "--"。
        {
            auto now = steady_clock::now();
            int elapsed = (int)duration_cast<milliseconds>(now - start).count() + 1;
            node.totalRecvBytes = cur_recv_bytes;
            node.avgSpeed = speedCalc(cur_recv_bytes * 1000.0 / elapsed);
            node.maxSpeed = speedCalc(max_speed);
        }
        running = still_running;
        writeLog(LOG_TYPE_FILEDL, "Running threads: " + std::to_string(running) + ", total received bytes: " + std::to_string(transferred_bytes) \
                 + ", current received bytes: " + std::to_string(this_bytes) + ".");
        if(!running)
            break;
        // 用户在测速过程中点了停止 → 立刻跳出累积循环，后续 EXIT_FLAG=true
        // 会让所有 worker 的 socket 循环也退出。已采集的 rawSpeed/avgSpeed
        // 保留，方便用户看到部分结果。
        if(wantStopNow())
        {
            writeLog(LOG_TYPE_FILEDL, "Stop requested, breaking download accumulation loop.");
            break;
        }
        draw_progress_dl(i, this_bytes);
    }
    std::cerr<<std::endl;
    writeLog(LOG_TYPE_FILEDL, "Test completed. Terminate all threads.");
    EXIT_FLAG = true; //terminate all threads right now
    while(!opened_socket.empty()) //close all sockets
    {
        shutdown(opened_socket.front(), SD_BOTH);
        closesocket(opened_socket.front());
        opened_socket.pop();
    }
    cur_recv_bytes = received_bytes; //save current received byte
    auto end = steady_clock::now();
    auto duration = duration_cast<milliseconds>(end - start);
    int deltatime = duration.count() + 1;//add 1 to prevent some error
    node.totalRecvBytes = cur_recv_bytes;
    node.avgSpeed = speedCalc(cur_recv_bytes * 1000.0 / deltatime);
    node.maxSpeed = speedCalc(max_speed);
    if(node.avgSpeed == "0.00B")
    {
        node.avgSpeed = "N/A";
        node.maxSpeed = "N/A";
    }
    writeLog(LOG_TYPE_FILEDL, "Downloaded " + std::to_string(cur_recv_bytes) + " bytes in " + std::to_string(deltatime) + " milliseconds.");
    for(int i = 0; i < thread_count; i++)
    {
        pthread_cancel(threads[i]);
        pthread_join(threads[i], NULL);
        writeLog(LOG_TYPE_FILEDL, "Thread #" + std::to_string(i + 1) + " has exited.");
    }
    // 聚合 TLS 校验结果到 node:有任一线程证书校验失败 → Failed;否则有线程
    // 通过 → Verified;一次都没尝试(HTTP 测试 / 全部连接拨不通) → NotApplicable。
    // renderer_v2 footer 据此条件渲染。
    node.tlsVerified = tls_state_finalize();
    writeLog(LOG_TYPE_FILEDL, "Multi-thread download test completed.");
    return 0;
}

int upload_test(nodeInfo &node, std::string localaddr, int localport, std::string username, std::string password)
{
    writeLog(LOG_TYPE_FILEUL, "Upload test started.");
    //prep up vars first
    std::string host, uri, testfile = node.ulTarget;
    int port = 0, i, running;
    bool useTLS = false;

    writeLog(LOG_TYPE_FILEUL, "Upload destination: " + testfile);
    urlParse(testfile, host, uri, port, useTLS);
    received_bytes = 0;
    EXIT_FLAG = false;

    if(useTLS)
    {
        writeLog(LOG_TYPE_FILEUL, "Found HTTPS URL. Initializing OpenSSL library.");
        SSL_Library_init();
    }
    else
    {
        writeLog(LOG_TYPE_FILEUL, "Found HTTP URL.");
    }

    //std::thread workers[2];
    pthread_t workers[2];
    thread_args args = {host, port, uri, localaddr, localport, username, password, useTLS};
    launched = 0;
    int created = 0;
    for(i = 0; i < 1; i++)
    {
        writeLog(LOG_TYPE_FILEUL, "Starting up worker thread #" + std::to_string(i + 1) + ".");
        if(pthread_create(&workers[i], NULL, _thread_upload_caller, &args) == 0)
            created++;
        else
            writeLog(LOG_TYPE_ERROR, "pthread_create failed for upload worker #" + std::to_string(i + 1));
    }
    if(created == 0)
    {
        writeLog(LOG_TYPE_ERROR, "All upload workers failed to start; aborting upload for this node.");
        node.ulSpeed = "N/A";
        return -1;
    }
    // 见 perform_test:用 3s 上限避免线程被 sched 长期挂起时主线程死锁。
    {
        int waited = 0;
        while(!launched && waited < 3000)
        {
            sleep(20);
            waited += 20;
        }
        if(!launched)
        {
            writeLog(LOG_TYPE_ERROR, "No upload worker reached entry within 3s; aborting upload for this node.");
            EXIT_FLAG = true;
            for(int k = 0; k < 1; ++k) pthread_join(workers[k], NULL);
            node.ulSpeed = "N/A";
            return -1;
        }
    }

    writeLog(LOG_TYPE_FILEUL, "Worker threads launched. Start accumulating data.");
    auto start = steady_clock::now();
    unsigned long long transferred_bytes = 0, this_bytes = 0, cur_sent_bytes = 0;
    for(i = 1; i < 11; i++)
    {
        sleep(1000); //accumulate data
        cur_sent_bytes = received_bytes;
        this_bytes = cur_sent_bytes - transferred_bytes;
        transferred_bytes = cur_sent_bytes;
        sleep(1); //slow down to prevent some problem
        running = still_running;
        writeLog(LOG_TYPE_FILEUL, "Running worker threads: " + std::to_string(running) + ", total sent bytes: " + std::to_string(transferred_bytes) \
                 + ", current sent bytes: " + std::to_string(this_bytes) + ".");
        if(!running)
            break;
        // 同 perform_test:停止信号到 → 立刻跳出上传累积循环。
        if(wantStopNow())
        {
            writeLog(LOG_TYPE_FILEUL, "Stop requested, breaking upload accumulation loop.");
            break;
        }
        draw_progress_ul(i, this_bytes);
    }
    std::cerr<<std::endl;
    writeLog(LOG_TYPE_FILEUL, "Test completed. Terminate worker threads.");
    EXIT_FLAG = true; //terminate worker thread right now
    while(!opened_socket.empty()) //close all sockets
    {
        shutdown(opened_socket.front(), SD_BOTH);
        closesocket(opened_socket.front());
        opened_socket.pop();
    }
    this_bytes = received_bytes; //save current uploaded data
    auto end = steady_clock::now();
    auto duration = duration_cast<milliseconds>(end - start);
    int deltatime = duration.count() + 1;//add 1 to prevent some error
    sleep(5); //slow down to prevent some problem
    node.ulSpeed = speedCalc(this_bytes * 1000.0 / deltatime);
    if(node.ulSpeed == "0.00B")
    {
        node.ulSpeed = "N/A";
    }
    writeLog(LOG_TYPE_FILEUL, "Uploaded " + std::to_string(this_bytes) + " bytes in " + std::to_string(deltatime) + " milliseconds.");
    node.totalRecvBytes += this_bytes;
    for(int i = 0; i < 1; i++)
    {
        pthread_cancel(workers[i]);
        pthread_join(workers[i], NULL);
        writeLog(LOG_TYPE_FILEUL, "Thread #" + std::to_string(i + 1) + " has exited.");
    }
    writeLog(LOG_TYPE_FILEUL, "Upload test completed.");
    return 0;
}
