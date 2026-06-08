#include <string>
#include <vector>
#include <numeric>

#include "ini_reader.h"
#include "misc.h"
#include "nodeinfo.h"
#include "printout.h"

// Import a previous run's result log (.log, an INI written by saveResult) so
// the renderer can re-export it. Protocol-agnostic: every node is a "LOG" unit
// that singleTest() short-circuits. Returns 0 on success, -1 if not a log file.
int explodeLog(const std::string &log, std::vector<nodeInfo> &nodes)
{
    INIReader ini;
    std::vector<std::string> nodeList, vArray;
    std::string strTemp;
    nodeInfo node;
    if(!startsWith(log, "[Basic]"))
        return -1;
    ini.Parse(log);

    if(!ini.SectionExist("Basic") || !ini.ItemExist("Basic", "GenerationTime") || !ini.ItemExist("Basic", "Tester"))
        return -1;

    nodeList = ini.GetSections();
    node.proxyStr = "LOG";
    for(auto &x : nodeList)
    {
        if(x == "Basic")
            continue;
        ini.EnterSection(x);
        vArray = split(x, "^");
        node.group = vArray[0];
        node.remarks = vArray[1];
        node.avgPing = ini.Get("AvgPing");
        node.avgSpeed = ini.Get("AvgSpeed");
        node.groupID = ini.GetNumber<int>("GroupID");
        node.id = ini.GetNumber<int>("ID");
        node.maxSpeed = ini.Get("MaxSpeed");
        node.online = ini.GetBool("Online");
        node.pkLoss = ini.Get("PkLoss");
        ini.GetNumberArray<int>("RawPing", ",", node.rawPing);
        ini.GetNumberArray<int>("RawSitePing", ",", node.rawSitePing);
        ini.GetNumberArray<unsigned long long>("RawSpeed", ",", node.rawSpeed);
        node.sitePing = ini.Get("SitePing");
        node.totalRecvBytes = ini.GetNumber<unsigned long long>("UsedTraffic");
        node.ulSpeed = ini.Get("ULSpeed");
        nodes.push_back(node);
    }

    return 0;
}