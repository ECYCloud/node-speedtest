#include <string>
#include <iostream>
#include <thread>
#include <mutex>
#include <algorithm>
#include <atomic>
#include <functional>

#include "webserver.h"
#include "misc.h"
#include "webget.h"
#include "version.h"
#include "nodeinfo.h"
#include "rapidjson_extra.h"
#include "renderer.h"
#include "speedtestutil.h"
#include "subconv.h"

std::atomic<bool> start_flag = false;
std::atomic<time_t> done_time = 0;
// 节点级中断信号:前端 POST /stop → 置 true → main.cpp::batchTest 在每个节点开始前检查,
// true 就 break 出循环并保留已测结果。/start 启动新一轮测速时复位为 false。
// 注意:单节点正在下载阶段无法立刻打断(没改 webget/socket 的循环),最多等当前节点结束。
// 这是"杀后端进程"路线被替换后的核心 —— 进程不再被杀,allNodes/targetNodes 全程保留,
// 用户停止后再点开始可以无缝复用同一份订阅与 mihomo outbound,不会再出现"任何节点都无法测试"。
std::atomic<bool> stop_requested = false;

//variables from main
extern std::vector<nodeInfo> allNodes;
extern int cur_node_id, socksport;
extern std::string speedtest_mode, export_sort_method, custom_group, override_conf_port;
extern string_array custom_exclude_remarks, custom_include_remarks;
extern unsigned int node_count;
// 本地 mihomo 内核版本(由 renderer.cpp 定义、main.cpp::initUserAgent 在启动时
// 通过 `mihomo -v` 读出);用于 /checkupdate 路由对比 GitHub 最新版本。
extern std::string mihomo_kernel_version;
// 共享版本比对函数(main.cpp 定义),vX.Y.Z 逐段数字比较。
int compareKernelVersion(const std::string &a, const std::string &b);

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

    // 防御:请求体缺 configs 或类型不对时直接返回，避免 json["configs"] 访问
    // 不存在的成员触发 rapidjson RAPIDJSON_ASSERT(本项目重定义为抛异常 → 崩溃)。
    if(!json.IsObject() || !json.HasMember("configs") || !json["configs"].IsArray())
    {
        node_count = 0;
        return;
    }

    // 单条解析失败不打断整批 —— 任何一条 configs[i] 不是 object、缺 config
    // 子对象、或 server_port 不是数字时，以前 stoi 直接抛 invalid_argument,
    // 异常上抛到 detached 线程的顶层 catch 让 batchTest 根本不会执行，前端
    // 表现为"点开始无反应"。这里收敛到单条:坏数据跳过，好数据继续匹配。
    auto safePort = [](const std::string &s) -> int {
        try { return s.empty() ? 0 : std::stoi(s); }
        catch(...) { return 0; }
    };
    for(unsigned int i = 0; i < json["configs"].Size(); i++)
    {
        const auto &item = json["configs"][i];
        if(!item.IsObject() || !item.HasMember("config") || !item["config"].IsObject())
            continue;
        const auto &cfg = item["config"];
        group = GetMember(cfg, "group");
        remarks = GetMember(cfg, "remarks");
        server = GetMember(cfg, "server");
        server_port = safePort(GetMember(cfg, "server_port"));
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
    // 安全转换:节点处于测试中间态时这些字段可能为空/非数字，裸 stod 会抛
    // invalid_argument 异常 —— /getresults 在 web 线程里调用，异常会让该线程崩溃。
    // 用带默认值的解析兜住，异常值一律按 0 处理。
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
    // 老写法 `counter / total * 1.0` 是 int 整除再升 double,小数位永远 0 或 1;
    // 且 rawSitePing 为空数组时 total=0 → 整数除 0 UB。改为先升 double 再除,
    // 并对 total==0 显式返 0。
    writer.Double(total > 0 ? static_cast<double>(counter) / total : 0.0);
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
    // UDP 支持等级:STUN / RFC 3489 检测出的 NAT 类型，前端结果表"UDP"列展示。
    // 字段值是 ntt.cpp NAT_TYPE_STR 中的英文枚举(FullCone / RestrictedCone /
    // PortRestrictedCone / Symmetric / Blocked / Unknown),前端再映射成中文。
    // 与历史记录 PNG 渲染走同一份数据源，语义一致。
    writer.Key("natType");
    writer.String(node.natType.get().data());
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
            if(x.id == cur_node_id && x.id != -1)
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
        if(x.id == -1)
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
        // 协议类型直接用内核回填的 proxy_type 字符串(Vless / Trojan / Hysteria2 …)。
        // 原本的 linkType switch 在协议重构后所有协议分支已成死代码，这里改为
        // "内核怎么报就怎么传给前端",新协议跟着 mihomo 自动支持。
        writer.Key("type");
        writer.String((x.proxy_type.empty() ? std::string("Unknown") : x.proxy_type).data());
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

// 通用 GitHub /releases/latest 检查 - mihomo 内核与软件自身两个 /check*update
// 路由共享同一套缓存 + 异步刷新 + 系统代理逻辑，这里集中实现避免重复一份。
struct UpdateCheckCache
{
    std::mutex mu;
    std::string latest;
    std::string error;
    bool has_update = false;
    time_t at = 0;
    std::atomic<bool> refreshing{false};
};

// local_provider 每次进路由时取一次本地版本：mihomo 路由读 mihomo_kernel_version
// (启动晚期才填好),软件自更新读 VERSION 宏(编译期常量)。
static std::string serveUpdateCheck(UpdateCheckCache &cache,
                                    const std::string &api_url,
                                    const std::string &release_url,
                                    std::function<std::string()> local_provider)
{
    static const time_t CACHE_TTL = 1800; // 30 分钟

    std::string local = local_provider();
    std::string latest, error;
    bool has_update = false;
    time_t at;
    bool stale;
    {
        std::lock_guard<std::mutex> lk(cache.mu);
        latest = cache.latest;
        error = cache.error;
        has_update = cache.has_update;
        at = cache.at;
        stale = (at == 0) || (time(NULL) - at > CACHE_TTL);
    }

    if(stale && !cache.refreshing.exchange(true))
    {
        std::thread([&cache, api_url, local_provider]()
        {
            std::string new_latest, new_error;
            bool new_has = false;
            try
            {
                // proxy 传空:libcurl 自动读 HTTPS_PROXY/HTTP_PROXY 环境变量。
                // Tauri 启动时把 Windows 系统代理注入进程 env,直连 GitHub 在国内
                // 经常超时，走代理后秒回。
                std::string body = webGet(api_url, "", 0);
                if(body.empty())
                    new_error = "GitHub API 无响应(网络受限或被防火墙拦截)";
                else
                {
                    rapidjson::Document j;
                    j.Parse(body.data());
                    if(j.HasParseError() || !j.HasMember("tag_name") || !j["tag_name"].IsString())
                        new_error = "GitHub API 返回了非预期的内容";
                    else
                    {
                        new_latest = j["tag_name"].GetString();
                        std::string local_now = local_provider();
                        if(!new_latest.empty() && !local_now.empty())
                            new_has = compareKernelVersion(new_latest, local_now) > 0;
                    }
                }
            }
            catch(const std::exception &e) { new_error = std::string("检查失败:") + e.what(); }
            catch(...) { new_error = "检查失败(未知异常)"; }
            {
                std::lock_guard<std::mutex> lk(cache.mu);
                cache.latest = new_latest;
                cache.error = new_error;
                cache.has_update = new_has;
                cache.at = time(NULL);
            }
            cache.refreshing.store(false);
        }).detach();
    }

    // 第一次访问 cache 还空，给前端温和的"正在检查"信号让其轮询。
    if(latest.empty() && error.empty() && at == 0)
        error = "正在检查...";

    rapidjson::StringBuffer sb;
    rapidjson::Writer<rapidjson::StringBuffer> w(sb);
    w.StartObject();
    w.Key("local");       w.String(local.data());
    w.Key("latest");      w.String(latest.data());
    w.Key("has_update");  w.Bool(has_update);
    w.Key("release_url"); w.String(release_url.data());
    w.Key("error");       w.String(error.data());
    w.EndObject();
    return sb.GetString();
}


void ssrspeed_webserver_routine(const std::string &listen_address, int listen_port)
{
    // listener_args: { addr, port, listen_backlog, worker_threads }
    //
    // worker_threads = 16:每个 worker 是独立 event_base 跑事件循环。订阅下载 /
    // mihomo 启动会同步阻塞 1 个 worker 5-30 秒,worker 越多，被全占的概率越低。
    // 旧值 4 在用户连点"加载订阅"时存在 worker 全占 → /getversion 排队 timeout
    // → 前端"后端未连接"的潜在风险。16 worker 实测内存增量 < 100KB,idle CPU 0,
    // 容量提到 4 倍后正常使用场景下不可能被全占。
    //
    // listen_backlog = 64:accept 排队上限。前端启动瞬间会并发发出 4-6 个请求
    // (StatusPill / 加载订阅 / 状态轮询 ...),64 给足缓冲。
    listener_args args = {listen_address, listen_port, 64, 16};
    extern bool gServeFile;
    extern std::string gServeFileRoot;
    gServeFile = true;
    gServeFileRoot = "webui/";

    append_response("GET", "/status", "text/plain", [](RESPONSE_CALLBACK_ARGS) -> std::string
    {
        return start_flag ? "running" : "stopped";
    });

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
        if(start_flag)
            return "running";
        // 协议重构后引擎不再解析 SS/SSR/v2rayN/SSD 等格式，本地配置导入仅支持
        // Clash YAML / base64 分享链接列表 / 单条分享链接(由 loadSubscription 识别)。
        // 0 节点 = 文件无法识别为上述任一形态。
        if(loadSubscription(getFormData(request.postdata), override_conf_port, allNodes) == 0)
            return "error";
        rewriteNodeID(allNodes);
        launchMihomoForNodes(allNodes); // 见 /readsubscriptions 注释
        // 同 /readsubscriptions:清空上一次测试结果，保证全新会话
        eraseElements(targetNodes);
        cur_node_id = -1;
        return ssrspeed_generate_web_configs(allNodes);
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
            // 复位上一轮残留的中断信号，确保新一轮 batchTest 不会在第一节点就被 break。
            stop_requested = false;
            // 顶层兜底:测速链路里任何一步抛异常(尤其 rapidjson 的 RAPIDJSON_ASSERT
            // 被重定义为 throw runtime_error —— GeoIP/内核返回非预期 JSON 时会触发),
            // 若不捕获就会让这个 detached 线程 terminate 掉整个后端进程，表现为"无法测试"。
            // 这里统一兜住:保证 start_flag 复位、后端存活，单个节点失败不影响整体。
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

    // 节点级停止:置 stop_requested = true,batchTest 当前节点跑完后跳出循环。
    // 同时把 done_time 清零，避免 /start 的"5 秒冷却"误挡停止后立刻重测。
    // 不杀后端进程 —— allNodes/targetNodes/mihomo outbound 全保留，再点开始可无缝继续。
    append_response("POST", "/stop", "text/plain", [](RESPONSE_CALLBACK_ARGS) -> std::string
    {
        (void)request;
        if(!start_flag)
            return "stopped";
        stop_requested = true;
        done_time = 0;
        response.status_code = 202;
        return "stopping";
    });

    append_response("GET", "/getresults", "text/plain;charset=utf-8", [](RESPONSE_CALLBACK_ARGS) -> std::string
    {
        return ssrspeed_generate_results(targetNodes);
    });

    // 异步检查 mihomo 内核更新 / 软件自更新:维持文件级 cache,handler 立刻返 cache,
    // cache 过期或缺失时由后台线程刷新。原先同步 webGet GitHub API 会阻塞
    // 当前 worker 线程最长 15s(国内网络通常受限),期间该 worker 上排队
    // 的 /getversion 直接 timeout → 前端"后端未连接"。
    append_response("GET", "/checkupdate", "application/json;charset=utf-8", [](RESPONSE_CALLBACK_ARGS) -> std::string
    {
        (void)request;
        static UpdateCheckCache cache;
        return serveUpdateCheck(
            cache,
            "https://api.github.com/repos/MetaCubeX/mihomo/releases/latest",
            "https://github.com/MetaCubeX/mihomo/releases/latest",
            [](){ return mihomo_kernel_version; });
    });

    // 软件自更新:与 mihomo 内核更新对称，前端设置页"软件更新"区直接展示
    // 本地 vs GitHub 最新版本，不依赖 tauri-plugin-updater 拿 latest tag。
    append_response("GET", "/checkappupdate", "application/json;charset=utf-8", [](RESPONSE_CALLBACK_ARGS) -> std::string
    {
        (void)request;
        static UpdateCheckCache cache;
        return serveUpdateCheck(
            cache,
            "https://api.github.com/repos/ECYCloud/node-speedtest/releases/latest",
            "https://github.com/ECYCloud/node-speedtest/releases/latest",
            [](){ return std::string(VERSION); });
    });

    std::cerr << "Node Speedtest " VERSION " Web server running @ http://" << listen_address << ":" << listen_port << std::endl;
    start_web_server_multi(&args);
}
