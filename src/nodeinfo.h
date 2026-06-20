#ifndef NODEINFO_H_INCLUDED
#define NODEINFO_H_INCLUDED

#include <string>
#include <future>
#include <atomic>

#include "geoip.h"
#include "misc.h"

// TLS 证书核实状态。仅描述"测速下载链路"对节点出口拿到的服务端证书是否
// 走完了 OpenSSL 内置的 X.509 信任链 + 主机名校验(SSL_VERIFY_PEER +
// SSL_get_verify_result + SSL_set1_host),与节点本身的代理协议握手无关。
//   NotApplicable: 节点未参与 HTTPS 测速(pingonly / 测试文件是 HTTP /
//                  线程根本没拨通),不能下结论，渲染层据此跳过。
//   Verified:     至少一条下载/上传线程完成了对端证书校验且主机名匹配。
//   Failed:       拨通后 TLS 握手或证书校验失败(任一线程失败即标 Failed,
//                 因为这表明该节点测速链路的安全性不可信)。
enum class TlsVerifyState
{
    NotApplicable = 0,
    Verified      = 1,
    Failed        = 2,
};

struct nodeInfo
{
    // Protocol type string reported by the mihomo kernel (e.g. "Vless",
    // "Trojan", "Hysteria2"). Filled from /providers/proxies after the kernel
    // parses the subscription, so new protocols need no C++ changes to display.
    std::string proxy_type;
    // Transient (subscription-load only): the verbatim unit handed to the
    // kernel — either a single-proxy Clash YAML block or a raw share link.
    // is_link_unit distinguishes the two so providers are split correctly.
    std::string raw_unit;
    bool is_link_unit = false;
    int id = -1;
    int groupID = -1;
    bool online = false;
    std::string group;
    std::string remarks;
    std::string server;
    int port = 0;
    std::string proxyStr;
    unsigned long long rawSpeed[20] = {};
    unsigned long long totalRecvBytes = 0;
    int duration = 0;
    std::string avgSpeed = "N/A";
    std::string maxSpeed = "N/A";
    std::string ulSpeed = "N/A";
    std::string pkLoss = "100.00%";
    int rawPing[6] = {};
    std::string avgPing = "0.00";
    int rawSitePing[10] = {};
    std::string sitePing = "0.00";
    std::string traffic;
    FutureHelper<geoIPInfo> inboundGeoIP;
    FutureHelper<geoIPInfo> outboundGeoIP;
    std::string testFile;
    std::string ulTarget;
    FutureHelper<std::string> natType {"Unknown"};
    // 测速链路 TLS 证书核实状态。perform_test 启动 worker 前重置为
    // NotApplicable,worker 命中 HTTPS 测试文件时由 _thread_download/upload
    // 在握手 + 验证完成后通过 multithread_test 内的原子计数器累积,perform_test
    // 末尾把汇总结果写回这里,renderer_v2 footer 据此条件渲染。
    // 用普通 enum 而非 std::atomic 是为了保持 nodeInfo 可拷贝(整段 batchTest
    // 多处按值传递);并发写入由 multithread_test 的文件作用域原子负责。
    TlsVerifyState tlsVerified = TlsVerifyState::NotApplicable;
};

#endif // NODEINFO_H_INCLUDED
