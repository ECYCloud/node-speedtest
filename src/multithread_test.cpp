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
    setTimeout(sHost, 5000);
    if(startConnect(sHost, localaddr, localport) == SOCKET_ERROR || connectSocks5(sHost, username, password) == -1 || connectThruSocks(sHost, host, port) == -1)
        return -1;

    if(useTLS)
    {
        SSL_CTX *ctx;
        SSL *ssl;

        ctx = SSL_CTX_new(TLS_client_method());
        if(ctx == NULL)
        {
            ERR_print_errors_fp(stderr);
            return -1;
        }
        defer(SSL_CTX_free(ctx);)

        ssl = SSL_new(ctx);
        defer(SSL_free(ssl);)
        SSL_set_fd(ssl, sHost);

        if(SSL_connect(ssl) != 1)
        {
            ERR_print_errors_fp(stderr);
        }
        else
        {
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
    setTimeout(sHost, 5000);
    if(startConnect(sHost, localaddr, localport) == SOCKET_ERROR || connectSocks5(sHost, username, password) == -1 || connectThruSocks(sHost, host, port) == -1)
        return -1;

    if(useTLS)
    {
        SSL_CTX *ctx;
        SSL *ssl;

        ctx = SSL_CTX_new(TLS_client_method());
        if(ctx == NULL)
        {
            ERR_print_errors_fp(stderr);
            return -1;
        }
        defer(SSL_CTX_free(ctx);)

        ssl = SSL_new(ctx);
        defer(SSL_free(ssl);)
        SSL_set_fd(ssl, sHost);

        if(SSL_connect(ssl) != 1)
        {
            ERR_print_errors_fp(stderr);
        }
        else
        {
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
    for(i = 0; i != thread_count; i++)
    {
        writeLog(LOG_TYPE_FILEDL, "Starting up thread #" + std::to_string(i + 1) + ".");
        //threads[i] = std::thread(_thread_download, host, port, uri, localaddr, localport, username, password, useTLS);
        pthread_create(&threads[i], NULL, _thread_download_caller, &args);
    }
    while(!launched)
        sleep(20); //wait until any one of the threads start up

    writeLog(LOG_TYPE_FILEDL, "All threads launched. Start accumulating data.");
    auto start = steady_clock::now();
    unsigned long long transferred_bytes = 0, this_bytes = 0, cur_recv_bytes = 0, max_speed = 0;
    // 峰值速度规范化:对齐 Speedtest/Cloudflare/LibreSpeed 的标准做法,用滑动窗口
    // 均值的历史最大值作为"最高速度",而不是单 0.5s 瞬时最大值。
    //   * 单点瞬时最大易被 TSO 聚合、慢启动末期 cwnd 翻倍、丢包后 RTO 大窗口重传等
    //     抖动"虚高"拉到代表不了节点真实带宽的尖峰
    //   * 与前端展示的"实时速度"(同样 2s 滑动均值)保持同尺度,语义对齐:
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
        // 用现有数据均值(避免被 0 拉低,前几格也参与得到最大值就是 OK 的)。
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
        // 用户在测速过程中点了停止 → 立刻跳出累积循环,后续 EXIT_FLAG=true
        // 会让所有 worker 的 socket 循环也退出。已采集的 rawSpeed/avgSpeed
        // 保留,方便用户看到部分结果。
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
    for(i = 0; i < 1; i++)
    {
        writeLog(LOG_TYPE_FILEUL, "Starting up worker thread #" + std::to_string(i + 1) + ".");
        //workers[i] = std::thread(_thread_upload, host, port, uri, localaddr, localport, username, password, useTLS);
        pthread_create(&workers[i], NULL, _thread_upload_caller, &args);
    }
    while(!launched)
        sleep(20); //wait until worker thread starts up

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
