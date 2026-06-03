// Standalone preview harness for exportRender(). Builds a handful of synthetic
// nodes that resemble a real run, then renders one PNG so we can eyeball the
// SSRSpeed-style layout without doing a live speedtest.
//
// Link against every stairspeedtest object EXCEPT main.cpp.obj (which owns the
// real main()). Run from a dir that contains tools/misc/WenQuanYiMicroHei-01.ttf.

#include <vector>
#include <string>
#include <cstring>
#include "../src/nodeinfo.h"
#include "../src/renderer.h"
#include "../src/printout.h"

extern int image_scale;
extern std::string export_sort_method_render;

static nodeInfo mk(int id, int type, const std::string &group,
                   const std::string &remarks, const std::string &http_ping,
                   const std::string &site_ping, const std::string &avg,
                   const std::string &mx, unsigned long long peak)
{
    nodeInfo n;
    n.id = id; n.groupID = 0; n.linkType = type; n.online = true;
    n.group = group; n.remarks = remarks;
    n.avgPing = http_ping; n.sitePing = site_ping;
    n.avgSpeed = avg; n.maxSpeed = mx;
    n.totalRecvBytes = peak * 8; n.duration = 12;
    for(int j = 0; j < 20; ++j)
        n.rawSpeed[j] = static_cast<unsigned long long>(peak * (0.3 + 0.7 * ((j * 7) % 20) / 20.0));
    return n;
}

int main()
{
    image_scale = 4; // HD
    std::vector<nodeInfo> nodes;
    std::string g = "3487-ECY";
    // Real UTF-8 regional-indicator flag emoji prefixes to test emoji->flag:
    const char *US = "\xF0\x9F\x87\xBA\xF0\x9F\x87\xB8";
    const char *JP = "\xF0\x9F\x87\xAF\xF0\x9F\x87\xB5";
    const char *HK = "\xF0\x9F\x87\xAD\xF0\x9F\x87\xB0";
    const char *TW = "\xF0\x9F\x87\xB9\xF0\x9F\x87\xBC";
    const char *DE = "\xF0\x9F\x87\xA9\xF0\x9F\x87\xAA";
    const char *SG = "\xF0\x9F\x87\xB8\xF0\x9F\x87\xAC";
    nodes.push_back(mk(0, SPEEDTEST_MESSAGE_FOUNDVLESS, g, std::string(US)+" 美国 A3 [IPv4]",      "120.4", "642.31", "98.40MB", "152.66MB",160ull*1024*1024));
    nodes.push_back(mk(1, SPEEDTEST_MESSAGE_FOUNDHY2,   g, std::string(JP)+" 日本 B1 [IPv4]",      "88.0",  "388.04", "61.20MB", "88.10MB",  92ull*1024*1024));
    nodes.push_back(mk(2, SPEEDTEST_MESSAGE_FOUNDTROJAN,g, std::string(HK)+" 香港 II A1 [CF 0.1x]","210.7", "1204.7", "12.80MB", "20.40MB",  21ull*1024*1024));
    nodes.push_back(mk(3, SPEEDTEST_MESSAGE_FOUNDVLESS, g, std::string(TW)+" 台湾 C2 [IPv4]",       "0.00",  "0.00",   "3.20MB",  "5.10MB",   5ull*1024*1024));
    nodes.push_back(mk(4, SPEEDTEST_MESSAGE_FOUNDANYTLS,g, std::string(DE)+" 德国 D1 [IPv4]",       "260.5", "910.50", "44.10MB", "70.30MB",  72ull*1024*1024));
    nodes.push_back(mk(5, SPEEDTEST_MESSAGE_FOUNDVLESS, g, std::string(SG)+" 新加坡 E1 [IPv4]",     "70.2",  "150.10", "120.5MB", "160.2MB", 170ull*1024*1024));

    std::string out = exportRender("results/preview.log", nodes, true,
                                   "rmaxspeed", "rainbow", true, true);
    printf("rendered: %s\n", out.c_str());
    return 0;
}
