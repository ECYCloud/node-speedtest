#ifndef MULTITHREAD_TEST_H_INCLUDED
#define MULTITHREAD_TEST_H_INCLUDED

#include <string>

#include "misc.h"
#include "nodeinfo.h"

int perform_test(nodeInfo &node, std::string localaddr, int localport, std::string username, std::string password, int thread_count);
int upload_test(nodeInfo &node, std::string localaddr, int localport, std::string username, std::string password);

// 仅延迟模式下补做一次 TLS 证书核实。perform_test 整段被跳过时,延迟探测
// 走 mihomo Clash API 拿不到 verify_result,导致 node.tlsVerified 永远 NotApplicable,
// footer "已核实 TLS 证书"那行无法触发。这里通过 socks5 经 mihomo 自己拨一次
// 到 host:host_port,只完成 TLS 握手 + 严格 PKI 校验后立即断开,把结果写回
// node.tlsVerified,与 perform_test 同口径,不下载任何内容。
// 返回 0 表示链路通,握手结果(成功/失败/cacert 不可用)已记入 node.tlsVerified;
// 返回 -1 表示连 socks5 都没拨通,该节点跳过(node.tlsVerified 保持原值)。
int verifyTlsForLatency(nodeInfo &node, const std::string &socks_addr, int socks_port,
                        const std::string &host, int host_port);

#endif // MULTITHREAD_TEST_H_INCLUDED
