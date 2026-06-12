#include <string>
#include <fstream>
#include <sstream>
#include <mutex>
#include <atomic>

#include "logger.h"
#include "version.h"
#include "misc.h"
#include "printout.h"

#include <sys/time.h>
#ifdef _WIN32
#include <direct.h>
#else
#include <sys/types.h>
#include <sys/stat.h>
#endif // _WIN32

typedef std::lock_guard<std::mutex> guarded_mutex;
std::mutex logger_mutex;

std::string curtime, result_content;
std::string resultPath, logPath;

// 单个 .log 文件磁盘上限(字节):超过即把当前 .log 改名为 .log.old(覆盖旧 .old)
// 后从 0 开始写。保证用户在前端"日志"页打开任意一条主日志时大小始终 ≤ 512 KB,
// 同时保留上一段为 .log.old 便于追溯。logger 是热路径,用 atomic 计数避免每写
// 一行都 stat 一次磁盘。
constexpr size_t kLogMaxBytes = 512 * 1024;
static std::atomic<size_t> g_log_bytes{0};

// 入文件的级别阈值:数字 ≤ 阈值才写。默认 INFO(=3) 让 DEBUG/VERBOSE 的高频
// 心跳(webserver Accept / handle_cmd 等,前端 polling 每 500ms 触发一次)
// 不入文件,只保留对排错有用的 FATAL/ERROR/WARNING/INFO。
// 需要开发期看心跳时改成 LOG_LEVEL_VERBOSE 即可。
static int g_log_level_threshold = LOG_LEVEL_INFO;

int makeDir(const char *path)
{
#ifdef _WIN32
    return mkdir(path);
#else
    return mkdir(path, 0755);
#endif // _WIN32
}

std::string getTime(int type)
{
    time_t lt;
    char tmpbuf[32], cMillis[7];
    std::string format;
    timeval tv;
    gettimeofday(&tv, NULL);
    snprintf(cMillis, 7, "%.6ld", (long)tv.tv_usec);
    lt = time(NULL);
    struct tm *local = localtime(&lt);
    switch(type)
    {
    case 1:
        format = "%Y%m%d-%H%M%S";
        break;
    case 2:
        format = "%Y/%m/%d %a %H:%M:%S." + std::string(cMillis);
        break;
    case 3:
        format = "%Y-%m-%d %H:%M:%S";
        break;
    }
    strftime(tmpbuf, 32, format.data(), local);
    return std::string(tmpbuf);
}

void logInit(bool rpcmode)
{
    curtime = getTime(1);
    logPath = "logs" PATH_SLASH + curtime + ".log";
    g_log_bytes.store(0, std::memory_order_relaxed);
    std::string log_header = "Stair Speedtest " VERSION " started in ";
    if(rpcmode)
        log_header += "GUI mode.";
    else
        log_header += "CLI mode.";
    writeLog(LOG_TYPE_INFO, log_header);
}

// 文件名安全化:把 Windows / 通用文件系统的非法字符替换成下划线，并去掉首尾空白。
// 仅供 results/ 目录下的文件名使用，不需要 URL 编码级别的转义。
//
// 注意:非 ASCII 字符(中文等)予以保留 —— 文件最终通过 misc.cpp 的 fileWrite /
// renderer 的临时名重命名走 _wfopen / MoveFileW(UTF-8→UTF-16)落盘，中文文件名
// 在 Windows 上能正确创建，不再走窄字符 fopen 的 ACP 解码导致的乱码路径。
static std::string sanitizeForFilename(const std::string &s)
{
    std::string out;
    out.reserve(s.size());
    for(char c : s)
    {
        unsigned char uc = static_cast<unsigned char>(c);
        if(c == '\\' || c == '/' || c == ':' || c == '*' || c == '?' ||
           c == '"' || c == '<' || c == '>' || c == '|' || uc < 0x20)
            out += '_';
        else
            out += c;
    }
    // 去掉首尾空白和点(Windows 不允许目录名以点结尾)
    while(!out.empty() && (out.back() == ' ' || out.back() == '.'))
        out.pop_back();
    while(!out.empty() && (out.front() == ' ' || out.front() == '.'))
        out.erase(out.begin(), out.begin() + 1);
    return out;
}

void resultInit(const std::string &group_name)
{
    curtime = getTime(1);
    std::string safe = sanitizeForFilename(group_name);
    if(safe.empty())
        resultPath = "results" PATH_SLASH + curtime + ".log";
    else
        resultPath = "results" PATH_SLASH + safe + "-" + curtime + ".log";
}

void writeLog(int type, std::string content, int level)
{
    // level 过滤放在拿锁之前 — 高频心跳(webserver Accept / handle_cmd)直接被
    // 短路掉,不抢 logger_mutex,也不影响主测试线程的吞吐。
    if(level > g_log_level_threshold)
        return;
    guarded_mutex guard(logger_mutex);
    std::string timestr = "[" + getTime(2) + "]", typestr = "[UNKNOWN]";
    switch(type)
    {
    case LOG_TYPE_ERROR:
        typestr = "[ERROR]";
        break;
    case LOG_TYPE_INFO:
        typestr = "[INFO]";
        break;
    case LOG_TYPE_RAW:
        typestr = "[RAW]";
        break;
    case LOG_TYPE_WARN:
        typestr = "[WARNING]";
        break;
    case LOG_TYPE_GEOIP:
        typestr = "[GEOIP]";
        break;
    case LOG_TYPE_FILEDL:
        typestr = "[FILEDL]";
        break;
    case LOG_TYPE_FILEUL:
        typestr = "[FILEUL]";
        break;
    case LOG_TYPE_RULES:
        typestr = "[RULES]";
        break;
    case LOG_TYPE_RENDER:
        typestr = "[RENDER]";
        break;
    case LOG_TYPE_STUN:
        typestr = "[STUN]";
    }
    content = timestr + typestr + content + "\n";
    // 若本次写入会让 .log 越过 512 KB,先把当前内容 rotate 到 .log.old(覆盖旧的)
    // 然后 .log 从 0 开始。rename 失败时回退到清空当前文件,保证大小不会失控。
    size_t cur = g_log_bytes.load(std::memory_order_relaxed);
    if(cur + content.size() > kLogMaxBytes && !logPath.empty())
    {
        std::string old_path = logPath + ".old";
        std::remove(old_path.c_str());
        if(std::rename(logPath.c_str(), old_path.c_str()) != 0)
            std::remove(logPath.c_str());
        g_log_bytes.store(0, std::memory_order_relaxed);
    }
    fileWrite(logPath, content, false);
    g_log_bytes.fetch_add(content.size(), std::memory_order_relaxed);
}

void logEOF()
{
    writeLog(LOG_TYPE_INFO,"Program terminated.");
    fileWrite(logPath, "--EOF--", false);
}
