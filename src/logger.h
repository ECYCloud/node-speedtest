#ifndef LOGGER_H_INCLUDED
#define LOGGER_H_INCLUDED

#include <string>

#include "misc.h"

enum
{
    LOG_TYPE_INFO,
    LOG_TYPE_ERROR,
    LOG_TYPE_RAW,
    LOG_TYPE_WARN,
    LOG_TYPE_FILEDL,
    LOG_TYPE_GEOIP,
    LOG_TYPE_RULES,
    LOG_TYPE_RENDER,
    LOG_TYPE_FILEUL,
    LOG_TYPE_STUN
};

enum
{
    LOG_LEVEL_FATAL,
    LOG_LEVEL_ERROR,
    LOG_LEVEL_WARNING,
    LOG_LEVEL_INFO,
    LOG_LEVEL_DEBUG,
    LOG_LEVEL_VERBOSE
};

extern std::string resultPath, logPath;

int makeDir(const char *path);
std::string getTime(int type);
void logInit(bool rpcmode);
// 初始化测试结果文件路径(results/<safe_group>-<time>.log)。
// group_name 留空时只用时间戳，保留旧行为。
void resultInit(const std::string &group_name = "");
// level 默认 INFO:webserver 接受连接 / 处理路由这类高频心跳显式传 DEBUG/VERBOSE
// 即可被 logger 过滤掉,避免 512 KB 主日志被本机心跳塞满,真正的 ERROR/WARN
// 来不及看就被 rotate 走。
void writeLog(int type, std::string content, int level = LOG_LEVEL_INFO);
void logEOF();

#endif // LOGGER_H_INCLUDED
