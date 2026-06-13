#ifndef RENDERER_H_INCLUDED
#define RENDERER_H_INCLUDED

#include <string>
#include <vector>

#include "misc.h"
#include "logger.h"
#include "nodeinfo.h"

#define MAX_NODES_COUNT 1024

struct color
{
    int red = 0;
    int green = 0;
    int blue = 0;
};

extern std::vector<color> colorgroup;
extern std::vector<int> bounds;
extern bool export_as_ssrspeed;
extern int image_scale;

// 测试机 / 测试时间相关全局，由 main.cpp 在 batchTest 开始时填充。
// renderer footer 直接读取;为空时对应行/字段不渲染，优雅降级。
extern std::string g_local_country;     // 国家(中文)
extern std::string g_local_region;      // 省份/地区
extern std::string g_local_city;        // 城市
extern std::string g_local_isp;         // 运营商
extern std::string g_test_start_time;   // "2025-10-31 18:30:42" 本地时区
extern std::string g_test_tz_label;     // "(UTC+08:00)"

std::string exportRender(std::string resultpath, std::vector<nodeInfo> &nodes, bool export_with_maxspeed, std::string export_sort_method, bool export_as_new_style, bool export_nat_type = true);

#endif // RENDERER_H_INCLUDED
