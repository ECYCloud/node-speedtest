#include <string>
#include <iostream>
#include <thread>
#include <mutex>
#include <algorithm>
#include <atomic>

#include "webserver.h"
#include "misc.h"
#include "webget.h"
#include "version.h"
#include "nodeinfo.h"
#include "rapidjson_extra.h"
#include "printout.h"
#include "renderer.h"
#include "speedtestutil.h"
#include "version.h"

std::atomic<bool> start_flag = false;
std::atomic<time_t> done_time = 0;

//variables from main
extern std::vector<nodeInfo> allNodes;
extern int cur_node_id, socksport;
extern std::string speedtest_mode, export_sort_method, custom_group, override_conf_port;
extern bool ssr_libev, ss_libev;
extern string_array custom_exclude_remarks, custom_include_remarks;
extern unsigned int node_count;

//functions from main
void addNodes(std::string link, bool multilink);
void rewriteNodeID(std::vector<nodeInfo> &nodes);
void batchTest(std::vector<nodeInfo> &nodes);
bool launchMihomoForNodes(std::vector<nodeInfo> &nodes);

//webui variables
std::vector<nodeInfo> targetNodes;
std::string server_status = "stopped";

nodeInfo find_node(std::string &group, std::string &remarks, std::string &server, int &server_port)
{
    auto iter = std::find_if(allNodes.begin(), allNodes.end(), [&](auto x){ return x.group == group && x.remarks == remarks && x.server == server && x.port == server_port; });
    if(iter != allNodes.end())
        return *iter;
    return nodeInfo();
}

void ssrspeed_regenerate_node_list(rapidjson::Document &json)
{
    nodeInfo node;
    std::string group, remarks, server;
    int server_port;

    eraseElements(targetNodes);

    // 防御:请求体缺 configs 或类型不对时直接返回,避免 json["configs"] 访问
    // 不存在的成员触发 rapidjson RAPIDJSON_ASSERT(本项目重定义为抛异常 → 崩溃)。
    if(!json.IsObject() || !json.HasMember("configs") || !json["configs"].IsArray())
    {
        node_count = 0;
        return;
    }

    for(unsigned int i = 0; i < json["configs"].Size(); i++)
    {
        group = GetMember(json["configs"][i]["config"], "group");
        remarks = GetMember(json["configs"][i]["config"], "remarks");
        server = GetMember(json["configs"][i]["config"], "server");
        server_port = stoi(GetMember(json["configs"][i]["config"], "server_port"));
        auto iter = std::find_if(allNodes.begin(), allNodes.end(), [&](auto x){ return x.group == group && x.remarks == remarks && x.server == server && x.port == server_port; });
        if(iter != allNodes.end())
            targetNodes.push_back(*iter);
    }
    node_count = json["configs"].Size();
    rewriteNodeID(targetNodes);
}

double ssrspeed_get_speed_number(const std::string &speed)
{
    if(speed == "N/A")
        return 0;

    return streamToInt(speed);
}

void json_write_node(rapidjson::Writer<rapidjson::StringBuffer> &writer, nodeInfo &node)
{
    geoIPInfo inbound = node.inboundGeoIP.get(), outbound = node.outboundGeoIP.get();
    int counter = 0, total = 0;
    // 安全转换:节点处于测试中间态时这些字段可能为空/非数字,裸 stod 会抛
    // invalid_argument 异常 —— /getresults 在 web 线程里调用,异常会让该线程崩溃。
    // 用带默认值的解析兜住,异常值一律按 0 处理。
    auto safeD = [](const std::string &s) -> double {
        try { return s.empty() ? 0.0 : std::stod(s); }
        catch(...) { return 0.0; }
    };
    double lossPct = node.pkLoss.size() > 1 ? safeD(node.pkLoss.substr(0, node.pkLoss.size() - 1)) : 0.0;
    writer.Key("group");
    writer.String(node.group.data());
    writer.Key("remarks");
    writer.String(node.remarks.data());
    writer.Key("loss");
    writer.Double(lossPct / 100.0);
    writer.Key("ping");
    writer.Double(safeD(node.avgPing) / 1000.0);
    writer.Key("gPing");
    writer.Double(safeD(node.sitePing) / 1000.0);
    writer.Key("rawSocketSpeed");
    writer.StartArray();
    for(auto &y : node.rawSpeed)
    {
        writer.Int(y);
    }
    writer.EndArray();
    writer.Key("rawTcpPingStatus");
    writer.StartArray();
    for(auto &y : node.rawPing)
    {
        writer.Double(y / 1000.0);
    }
    writer.EndArray();
    writer.Key("rawGooglePingStatus");
    writer.StartArray();
    counter = total = 0;
    for(auto &y : node.rawSitePing)
    {
        total++;
        writer.Double(y / 1000.0);
        if(y == 0)
            counter++;
    }
    writer.EndArray();
    writer.Key("gPingLoss");
    writer.Double(counter / total * 1.0);
    writer.Key("webPageSimulation");
    writer.String("N/A");
    writer.Key("geoIP");
    writer.StartObject();
    writer.Key("inbound");
    writer.StartObject();
    writer.Key("address");
    writer.String(std::string(node.server + ":" + std::to_string(node.port)).data());
    writer.Key("info");
    writer.String(std::string((inbound.country.size() ? inbound.country : std::string("N/A")) + " " + \
                              (inbound.city.size() ? inbound.city : std::string("N/A")) + ", " + \
                              (inbound.organization.size() ? inbound.organization : std::string("N/A"))).data());
    writer.EndObject();
    writer.Key("outbound");
    writer.StartObject();
    writer.Key("address");
    writer.String(outbound.ip.data());
    writer.Key("info");
    writer.String(std::string((outbound.country.size() ? outbound.country : std::string("N/A")) + " " + \
                              (outbound.city.size() ? outbound.city : std::string("N/A")) + ", " + \
                              (outbound.organization.size() ? outbound.organization : std::string("N/A"))).data());
    writer.EndObject();
    writer.EndObject();
    writer.Key("dspeed");
    writer.Double(ssrspeed_get_speed_number(node.avgSpeed));
    // 最高瞬时速度，与 dspeed 同形，前端结果表展示"最高速度"列。
    writer.Key("dspeedMax");
    writer.Double(ssrspeed_get_speed_number(node.maxSpeed));
    writer.Key("trafficUsed");
    writer.Int(node.totalRecvBytes);
}

std::string ssrspeed_generate_results(std::vector<nodeInfo> &nodes)
{
    rapidjson::StringBuffer sb;
    rapidjson::Writer<rapidjson::StringBuffer> writer(sb);
    bool running = start_flag;

    writer.StartObject();
    writer.Key("status");
    writer.String(running ? "running" : "stopped");

    // current:正在测试的节点(仅运行中且 cur_node_id 命中)。其 avgSpeed/maxSpeed
    // 在 perform_test 下载循环内实时更新，前端据此显示实时速度。
    writer.Key("current");
    writer.StartObject();
    if(running)
    {
        for(nodeInfo &x : nodes)
        {
            if(x.id == cur_node_id && x.linkType != -1)
            {
                json_write_node(writer, x);
                break;
            }
        }
    }
    writer.EndObject();

    // results:已测完的节点 —— 直接按节点真实状态判定，不再依赖前端轮询的副作用
    // 累积已测列表(旧逻辑会漏掉最后一个节点、且测速快于轮询间隔时整段丢失)。
    // 测试进行中:id < cur_node_id 即已测完(节点按 id 顺序串行测试);
    // 测试结束(start_flag=false):全部节点都已完成。
    writer.Key("results");
    writer.StartArray();
    for(nodeInfo &x : nodes)
    {
        if(x.linkType == -1)
            continue;
        bool done = running ? (x.id < cur_node_id) : true;
        if(done)
        {
            writer.StartObject();
            json_write_node(writer, x);
            writer.EndObject();
        }
    }
    writer.EndArray();
    writer.EndObject();
    return sb.GetString();
}

std::string ssrspeed_generate_web_configs(std::vector<nodeInfo> &nodes)
{
    rapidjson::StringBuffer sb;
    rapidjson::Writer<rapidjson::StringBuffer> writer(sb);
    writer.StartArray();
    for(nodeInfo &x : nodes)
    {
        writer.StartObject();
        writer.Key("type");
        switch(x.linkType)
        {
        case SPEEDTEST_MESSAGE_FOUNDSS:
            writer.String("Shadowsocks");
            break;
        case SPEEDTEST_MESSAGE_FOUNDSSR:
            writer.String("ShadowsocksR");
            break;
        case SPEEDTEST_MESSAGE_FOUNDVMESS:
            writer.String("V2Ray");
            break;
        case SPEEDTEST_MESSAGE_FOUNDTROJAN:
            writer.String("Trojan");
            break;
        case SPEEDTEST_MESSAGE_FOUNDSNELL:
            writer.String("Snell");
            break;
        case SPEEDTEST_MESSAGE_FOUNDSOCKS:
            writer.String("Socks5");
            break;
        case SPEEDTEST_MESSAGE_FOUNDHTTP:
            writer.String("HTTP");
            break;
        case SPEEDTEST_MESSAGE_FOUNDVLESS:
            writer.String("VLESS");
            break;
        case SPEEDTEST_MESSAGE_FOUNDHY2:
            writer.String("Hysteria2");
            break;
        case SPEEDTEST_MESSAGE_FOUNDANYTLS:
            writer.String("AnyTLS");
            break;
        case SPEEDTEST_MESSAGE_FOUNDTUIC:
            writer.String("TUIC");
            break;
        case SPEEDTEST_MESSAGE_FOUNDHYSTERIA:
            writer.String("Hysteria");
            break;
        case SPEEDTEST_MESSAGE_FOUNDWIREGUARD:
            writer.String("WireGuard");
            break;
        case SPEEDTEST_MESSAGE_FOUNDSSH:
            writer.String("SSH");
            break;
        case SPEEDTEST_MESSAGE_FOUNDMIERU:
            writer.String("Mieru");
            break;
        case SPEEDTEST_MESSAGE_FOUNDSHADOWTLS:
            writer.String("ShadowTLS");
            break;
        default:
            writer.String("Unknown");
            writer.EndObject();
            continue;
        }
        writer.Key("config");
        writer.StartObject();
        writer.Key("group");
        writer.String(x.group.data());
        writer.Key("remarks");
        writer.String(x.remarks.data());
        writer.Key("server_port");
        writer.Int(x.port);
        writer.Key("server");
        writer.String(x.server.data());
        writer.EndObject();
        writer.EndObject();
    }
    writer.EndArray();
    return sb.GetString();
}

void ssrspeed_webserver_routine(const std::string &listen_address, int listen_port)
{
    listener_args args = {listen_address, listen_port, 10, 4};
    extern bool gServeFile;
    extern std::string gServeFileRoot;
    gServeFile = true;
    gServeFileRoot = "webui/";

    append_response("GET", "/status", "text/plain", [](RESPONSE_CALLBACK_ARGS) -> std::string
    {
        return start_flag ? "running" : "stopped";
    });

    /*append_response("GET", "/", "REDIRECT", [](RESPONSE_CALLBACK_ARGS) -> std::string
    {
        return "http://web1.ospf.in/";
    });*/

    append_response("GET", "/favicon.ico", "x-icon", [](RESPONSE_CALLBACK_ARGS) -> std::string
    {
        return fileGet("tools/gui/favicon.ico", true);
    });

    append_response("GET", "/getversion", "text/plain", [](RESPONSE_CALLBACK_ARGS) -> std::string
    {
        return "{\"main\":\"" VERSION "\",\"webapi\":\"0.6.1\"}";
    });

    append_response("POST", "/readsubscriptions", "text/plain;charset=utf-8", [](RESPONSE_CALLBACK_ARGS) -> std::string
    {
        if(start_flag)
            return "running";
        rapidjson::Document json;
        std::string suburl;
        json.Parse(request.postdata.data());
        suburl = GetMember(json, "url");
        eraseElements(allNodes);
        addNodes(suburl, false);
        rewriteNodeID(allNodes);
        // 关键修复:webserver 模式过去漏了这一步 ——
        // 把所有节点打包到 config.yaml 启动一次 mihomo，并把每个 node.proxyStr
        // 重写为 "node-N"。否则后续 batchTest 会把整个 yaml 当节点名传给
        // mihomoSwitchProxy，导致 outbound 切不到节点，全部 socks5 connect not accepted。
        launchMihomoForNodes(allNodes);
        // 读取新订阅即一次全新会话:清空上一次的测试结果状态，否则前端轮询 /getresults
        // 会把旧结果拉回来 —— 表现为"测试结果不重置""开始测速按钮闪回重新测速"。
        eraseElements(targetNodes);
        cur_node_id = -1;
        return ssrspeed_generate_web_configs(allNodes);
    });

    append_response("POST", "/readfileconfig", "text/plain", [](RESPONSE_CALLBACK_ARGS) -> std::string
    {
        eraseElements(allNodes);
        //fileWrite("received.txt", getFormData(postdata), true);
        if(start_flag)
            return "running";
        else
        {
            if(explodeConfContent(getFormData(request.postdata), override_conf_port, ss_libev, ssr_libev, allNodes) == SPEEDTEST_ERROR_UNRECOGFILE)
                return "error";
            else
            {
                rewriteNodeID(allNodes);
                launchMihomoForNodes(allNodes); // 见 /readsubscriptions 注释
                // 同 /readsubscriptions:清空上一次测试结果，保证全新会话
                eraseElements(targetNodes);
                cur_node_id = -1;
                return ssrspeed_generate_web_configs(allNodes);
            }
        }
    });

    append_response("POST", "/start", "text/plain", [](RESPONSE_CALLBACK_ARGS) -> std::string
    {
        if(start_flag)
            return "running";
        time_t cur_time = time(NULL);
        if(cur_time - done_time < 5)
            return "done";
        std::thread t([=]()
        {
            start_flag = true;
            // 顶层兜底:测速链路里任何一步抛异常(尤其 rapidjson 的 RAPIDJSON_ASSERT
            // 被重定义为 throw runtime_error —— GeoIP/内核返回非预期 JSON 时会触发),
            // 若不捕获就会让这个 detached 线程 terminate 掉整个后端进程,表现为"无法测试"。
            // 这里统一兜住:保证 start_flag 复位、后端存活,单个节点失败不影响整体。
            try
            {
                rapidjson::Document json;
                json.Parse(request.postdata.data());
                std::string test_mode = GetMember(json, "testMode"), sort_method = GetMember(json, "sortMethod"), group = GetMember(json, "group");

                if(test_mode == "ALL")
                    speedtest_mode = "all";
                else if(test_mode == "TCP_PING")
                    speedtest_mode = "pingonly";
                std::transform(sort_method.begin(), sort_method.end(), sort_method.begin(), ::tolower);
                export_sort_method = replace_all_distinct(sort_method, "reverse_", "r");
                custom_group = group;

                ssrspeed_regenerate_node_list(json);
                batchTest(targetNodes);
            }
            catch(const std::exception &e)
            {
                writeLog(LOG_TYPE_ERROR, std::string("Speedtest thread caught exception: ") + e.what());
                std::cerr << "测速过程出现异常已被捕获，后端继续运行：" << e.what() << std::endl;
            }
            catch(...)
            {
                writeLog(LOG_TYPE_ERROR, "Speedtest thread caught unknown exception.");
                std::cerr << "测速过程出现未知异常已被捕获，后端继续运行。" << std::endl;
            }
            done_time = time(NULL);
            start_flag = false;
        });
        t.detach();
        response.status_code = 202;
        return "running";
    });

    append_response("GET", "/getresults", "text/plain;charset=utf-8", [](RESPONSE_CALLBACK_ARGS) -> std::string
    {
        return ssrspeed_generate_results(targetNodes);
    });

    std::cerr << "Stair Speedtest " VERSION " Web server running @ http://" << listen_address << ":" << listen_port << std::endl;
    start_web_server_multi(&args);
}
