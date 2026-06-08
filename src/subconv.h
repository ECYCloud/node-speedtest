#ifndef SUBCONV_H_INCLUDED
#define SUBCONV_H_INCLUDED

#include <string>
#include <vector>

#include "nodeinfo.h"

// =============================================================================
// Subscription normalization for the mihomo-native pipeline.
//
// stair no longer parses protocol-specific fields. It only recognizes three
// mihomo-ingestible subscription shapes and hands the raw payload to the
// kernel via a `file` proxy-provider:
//   1. Clash YAML  (contains a `proxies:` / `Proxy:` list)
//   2. base64 share-link list (decodes to lines of vless:// / trojan:// / ...)
//   3. a single share link
//
// For each node we read ONLY the universal `name` / `server` / `port` (for
// GeoIP + result display); every protocol-specific field stays inside the
// verbatim unit the kernel parses. Anything that is not one of the three
// shapes above is rejected (no sing-box / Surge / Loon / QuantumultX support).
// =============================================================================

// Parse subscription `content` into preliminary nodes (appended to `nodes`).
// Each node carries a transient `raw_unit` (a single-proxy Clash YAML block or
// a raw share link) plus best-effort remarks/server/port. `custom_port`, when
// non-empty, overrides the node port. Returns the number of nodes added.
int loadSubscription(const std::string &content, const std::string &custom_port,
                     std::vector<nodeInfo> &nodes);

// Split surviving `nodes` into mihomo `file` proxy-providers (one for YAML
// proxy units, one for share-link units) and emit the full config.yaml text.
//
// `socks_port` / `controller` are substituted into the kernel config. Provider
// files to write are returned as parallel (path, content) vectors. The ordered
// node pointers per provider are recorded in `yaml_order` / `link_order` so the
// caller can zip them against the kernel's /providers/proxies response to fill
// each node's authoritative name + type after boot.
struct ProvidersBuild
{
    std::string config_yaml;
    std::string yaml_provider_path;   // empty if no YAML-unit nodes
    std::string yaml_provider_body;
    std::string link_provider_path;   // empty if no link-unit nodes
    std::string link_provider_body;
    std::vector<nodeInfo*> yaml_order;
    std::vector<nodeInfo*> link_order;
};

ProvidersBuild buildProvidersConfig(std::vector<nodeInfo> &nodes,
                                    int socks_port,
                                    const std::string &controller);

#endif // SUBCONV_H_INCLUDED
