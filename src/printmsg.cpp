#include <iostream>
#include <fstream>
#include <string>
#include <vector>

#include "printout.h"
#include "version.h"

//define print-out messages
struct LOOKUP_ITEM
{
    int index;
    std::string info;
};

LOOKUP_ITEM SPEEDTEST_MESSAGES[] =
{
    {SPEEDTEST_MESSAGE_EOF, "\n测速完成，按任意键退出..."},
    {SPEEDTEST_MESSAGE_WELCOME, "欢迎使用 Stair Speedtest " VERSION "！\n本程序基于 mihomo 内核，支持 VLESS / Hysteria2 / Trojan / AnyTLS 等协议的单链接和订阅链接。\n如需测试多条链接，请用 '|' 分隔。\n请输入链接： "},
    {SPEEDTEST_MESSAGE_MULTILINK, "检测到多条链接，正在解析所有节点。\n\n"},
    {SPEEDTEST_MESSAGE_FOUNDSUB, "检测到订阅链接。\n"},
    {SPEEDTEST_MESSAGE_FOUNDLOCAL, "检测到本地配置文件。\n"},
    {SPEEDTEST_MESSAGE_GROUP, "如果你导入的订阅链接没有附带分组名，可以在下方自定义分组名。\n如果链接里已经包含分组名，直接回车跳过即可。\n自定义分组名： "},
    {SPEEDTEST_MESSAGE_GOTSERVER, "\n当前节点 - 分组：?1?  备注：?2?  序号：?0?/?3?\n"},
    {SPEEDTEST_MESSAGE_STARTPING, "正在测试 HTTP 延迟...\n"},
    {SPEEDTEST_MESSAGE_STARTGEOIP, "正在解析 GeoIP 信息...\n"},
    {SPEEDTEST_MESSAGE_STARTGPING, "正在测试 HTTPS 延迟...\n"},
    {SPEEDTEST_MESSAGE_STARTSPEED, "正在执行下载速度测试...\n"},
    {SPEEDTEST_MESSAGE_STARTUPD, "正在执行上传速度测试...\n"},
    {SPEEDTEST_MESSAGE_GOTRESULT, "测试结果 - 平均速度：?0?  最大速度：?1?  上传速度：?2?  丢包率：?3?  HTTP 延迟：?4?  HTTPS 延迟：?5?  NAT 类型：?6?\n"},
    {SPEEDTEST_MESSAGE_TRAFFIC, "已使用流量：?traffic?\n"},
    {SPEEDTEST_MESSAGE_PICSAVING, "正在导出结果图片...\n"},
    {SPEEDTEST_MESSAGE_PICSAVINGMULTI, "正在为分组 ?0? 导出结果图片...\n"},
    {SPEEDTEST_MESSAGE_PICSAVED, "结果图片已保存到 \"?0?\"。\n"},
    {SPEEDTEST_MESSAGE_PICSAVEDMULTI, "分组 ?0? 的结果图片已保存到 \"?1?\"。\n"},
    {SPEEDTEST_MESSAGE_FETCHSUB, "正在下载订阅数据...\n"},
    {SPEEDTEST_MESSAGE_PARSING, "正在解析配置文件...\n"},
    {SPEEDTEST_MESSAGE_BEGIN, "测速即将开始。\n"},
    {SPEEDTEST_MESSAGE_GOTGEOIP, "解析到出口服务器 ISP：?1?  国家代码：?2?\n"},
    {SPEEDTEST_MESSAGE_STARTNAT, "正在执行 UDP NAT 类型测试...\n"},
    {SPEEDTEST_MESSAGE_GOTNAT, "UDP NAT 类型测试结果：?1?\n"},
    {SPEEDTEST_ERROR_UNDEFINED, "未知错误！\n"},
    {SPEEDTEST_ERROR_WSAERR, "WSA 启动错误！\n"},
    {SPEEDTEST_ERROR_SOCKETERR, "Socket 错误！\n"},
    {SPEEDTEST_ERROR_NORECOGLINK, "未识别到有效链接，请检查输入。\n"},
    {SPEEDTEST_ERROR_UNRECOGFILE, "无法识别此配置文件。请确认文件为 Shadowsocks / ShadowsocksR / v2rayN 配置或标准订阅文件。\n"},
    {SPEEDTEST_ERROR_NOCONNECTION, "无法连接到服务器。\n"},
    {SPEEDTEST_ERROR_INVALIDSUB, "订阅链接没有返回任何内容，请检查链接是否正确。\n"},
    {SPEEDTEST_ERROR_NONODES, "没有解析到任何节点，请检查订阅链接。\n"},
    {SPEEDTEST_ERROR_NORESOLVE, "无法解析服务器地址。\n"},
    {SPEEDTEST_ERROR_RETEST, "本次测速无速度，正在重新测试...\n"},
    {SPEEDTEST_ERROR_NOSPEED, "连续两次测速均无速度，跳过该节点...\n"},
    {SPEEDTEST_ERROR_SUBFETCHERR, "直连方式无法获取订阅数据，正在尝试使用系统代理...\n"},
    {SPEEDTEST_ERROR_GEOIPERR, "无法获取 GeoIP 信息，已跳过。\n"}
};

LOOKUP_ITEM SPEEDTEST_MESSAGES_RPC[] =
{
    {SPEEDTEST_MESSAGE_WELCOME, "{\"info\":\"started\"}\n"},
    {SPEEDTEST_MESSAGE_EOF, "{\"info\":\"eof\"}\n"},
    {SPEEDTEST_MESSAGE_FOUNDSUB, "{\"info\":\"foundsub\"}\n"},
    {SPEEDTEST_MESSAGE_FOUNDLOCAL, "{\"info\":\"foundlocal\"}\n"},
    {SPEEDTEST_MESSAGE_FOUNDUPD, "{\"info\":\"foundupd\"}\n"},
    {SPEEDTEST_MESSAGE_GOTSERVER, "{\"info\":\"gotserver\",\"id\":?0?,\"group\":\"?1?\",\"remarks\":\"?2?\"}\n"},
    {SPEEDTEST_MESSAGE_STARTPING, "{\"info\":\"startping\",\"id\":?0?}\n"},
    {SPEEDTEST_MESSAGE_GOTPING, "{\"info\":\"gotping\",\"id\":?0?,\"ping\":\"?1?\",\"loss\":\"?2?\"}\n"},
    {SPEEDTEST_MESSAGE_STARTGEOIP, "{\"info\":\"startgeoip\",\"id\":?0?}\n"},
    {SPEEDTEST_MESSAGE_GOTGEOIP, "{\"info\":\"gotgeoip\",\"id\":?0?,\"isp\":\"?1?\",\"location\":\"?2?\"}\n"},
    {SPEEDTEST_MESSAGE_STARTNAT, "{\"info\":\"startnat\",\"id\":?0?}\n"},
    {SPEEDTEST_MESSAGE_GOTNAT, "{\"info\":\"gotnat\",\"id\":?0?,\"result\":\"?1?\"}\n"},
    {SPEEDTEST_MESSAGE_STARTSPEED, "{\"info\":\"startspeed\",\"id\":?0?}\n"},
    {SPEEDTEST_MESSAGE_GOTSPEED, "{\"info\":\"gotspeed\",\"id\":?0?,\"speed\":\"?1?\",\"maxspeed\":\"?2?\"}\n"},
    {SPEEDTEST_MESSAGE_STARTUPD, "{\"info\":\"startupd\",\"id\":?0?}\n"},
    {SPEEDTEST_MESSAGE_GOTUPD, "{\"info\":\"gotupd\",\"id\":?0?,\"ulspeed\":\"?1?\"}\n"},
    {SPEEDTEST_MESSAGE_STARTGPING, "{\"info\":\"startgping\",\"id\":?0?}\n"},
    {SPEEDTEST_MESSAGE_GOTGPING, "{\"info\":\"gotgping\",\"id\":?0?,\"ping\":\"?1?\"}\n"},
    {SPEEDTEST_MESSAGE_TRAFFIC, "(\"info\":\"traffic\",\"size\":\"?0?\"}\n"},
    {SPEEDTEST_MESSAGE_PICSAVING, "{\"info\":\"picsaving\"}\n"},
    {SPEEDTEST_MESSAGE_PICSAVED, "{\"info\":\"picsaved\",\"path\":\"?0?\"}\n"},
    {SPEEDTEST_MESSAGE_PICSAVEDMULTI, "{\"info\":\"picsaved\",\"path\":\"?0?\"}\n"},
    {SPEEDTEST_MESSAGE_FETCHSUB, "{\"info\":\"fetchingsub\"}\n"},
    {SPEEDTEST_MESSAGE_PARSING, "{\"info\":\"parsing\"}\n"},
    {SPEEDTEST_MESSAGE_BEGIN, "{\"info\":\"begintest\"}\n"},
    {SPEEDTEST_MESSAGE_PICDATA, "{\"info\":\"picdata\",\"data\":\"?0?\"}\n"},
    {SPEEDTEST_ERROR_UNDEFINED, "{\"info\":\"error\",\"reason\":\"undef\"}\n"},
    {SPEEDTEST_ERROR_WSAERR, "{\"info\":\"error\",\"reason\":\"wsaerr\"}\n"},
    {SPEEDTEST_ERROR_SOCKETERR, "{\"info\":\"error\",\"reason\":\"socketerr\"}\n"},
    {SPEEDTEST_ERROR_NORECOGLINK, "{\"info\":\"error\",\"reason\":\"norecoglink\"}\n"},
    {SPEEDTEST_ERROR_UNRECOGFILE, "{\"info\":\"error\",\"reason\":\"unrecogfile\"}\n"},
    {SPEEDTEST_ERROR_NOCONNECTION, "{\"info\":\"error\",\"reason\":\"noconnection\",\"id\":?0?}\n"},
    {SPEEDTEST_ERROR_INVALIDSUB, "{\"info\":\"error\",\"reason\":\"invalidsub\"}\n"},
    {SPEEDTEST_ERROR_NONODES, "{\"info\":\"error\",\"reason\":\"nonodes\"}\n"},
    {SPEEDTEST_ERROR_NORESOLVE, "{\"info\":\"error\",\"reason\":\"noresolve\",\"id\":?0?}\n"},
    {SPEEDTEST_ERROR_RETEST, "{\"info\":\"error\",\"reason\":\"retest\",\"id\":?0?}\n"},
    {SPEEDTEST_ERROR_NOSPEED, "{\"info\":\"error\",\"reason\":\"nospeed\",\"id\":?0?}\n"},
    {SPEEDTEST_ERROR_SUBFETCHERR, "{\"info\":\"error\",\"reason\":\"subfetcherr\"}\n"},
    {SPEEDTEST_ERROR_GEOIPERR, "{\"info\":\"error\",\"reason\":\"geoiperr\",\"id\":?0?}\n"}
};

std::string lookUp(int index, LOOKUP_ITEM *items)
{
    int i = 0;
    while (0 <= items[i].index)
    {
        if (items[i].index == index)
            return items[i].info;
        i++;
    }
    return std::string("");
}

std::string lookUp(int index, bool rpcmode)
{
    if(rpcmode)
        return lookUp(index, SPEEDTEST_MESSAGES_RPC);
    else
        return lookUp(index, SPEEDTEST_MESSAGES);
}
