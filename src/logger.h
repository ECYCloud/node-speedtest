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
void writeLog(int type, std::string content, int level = LOG_LEVEL_VERBOSE);
void logEOF();

#endif // LOGGER_H_INCLUDED
