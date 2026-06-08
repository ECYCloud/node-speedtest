#include <sstream>
#include <algorithm>

#include "yamlcpp_extra.h"
#include "misc.h"
#include "logger.h"
#include "subconv.h"
#include "webget.h"

using namespace YAML;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// Pull host/port/remark from a generic share-link URI. Universal fields only:
// scheme://<creds>@<host>:<port>...#<remark>. vmess:// (base64 JSON) has no
// host in the URI, so server/port stay empty there — the kernel still parses
// it and we reconcile the name afterwards.
static void linkMeta(const std::string &link, std::string &server,
                     std::string &port, std::string &remark)
{
    std::string body = link;
    string_size scheme = body.find("://");
    if(scheme != body.npos)
        body = body.substr(scheme + 3);

    string_size hash = body.find('#');
    if(hash != body.npos)
    {
        remark = UrlDecode(body.substr(hash + 1));
        body = body.substr(0, hash);
    }
    string_size q = body.find('?');
    if(q != body.npos)
        body = body.substr(0, q);
    string_size at = body.rfind('@');
    if(at != body.npos)
        body = body.substr(at + 1);
    // strip trailing path
    string_size slash = body.find('/');
    if(slash != body.npos)
        body = body.substr(0, slash);
    // host:port (handle IPv6 [::]:port)
    string_size colon = body.rfind(':');
    if(colon != body.npos && body.find(']') == body.npos)
    {
        server = body.substr(0, colon);
        port = body.substr(colon + 1);
    }
    else if(body.size() && body.front() == '[')
    {
        string_size rb = body.find(']');
        if(rb != body.npos)
        {
            server = body.substr(1, rb - 1);
            if(rb + 2 <= body.size() && body[rb + 1] == ':')
                port = body.substr(rb + 2);
        }
    }
    else
        server = body;
}

// Recognized share-link scheme? (everything the kernel can ingest from a
// link list — we don't enumerate to stay protocol-agnostic, just require a
// "<scheme>://" shape that isn't http/https sub URL.)
static bool isShareLink(const std::string &s)
{
    string_size p = s.find("://");
    if(p == s.npos || p == 0)
        return false;
    std::string scheme = toLower(s.substr(0, p));
    if(scheme == "http" || scheme == "https")
        return false;
    for(char c : scheme)
        if(!((c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '+' || c == '-' || c == '.'))
            return false;
    return true;
}

// Add one share-link node. raw_unit = the link itself (kernel parses it).
static void addLinkNode(const std::string &link, const std::string &custom_port,
                        std::vector<nodeInfo> &nodes)
{
    nodeInfo node;
    std::string server, port, remark;
    linkMeta(link, server, port, remark);
    if(!custom_port.empty())
        port = custom_port;
    node.server = server;
    node.port = to_int(port, 0);
    node.remarks = remark.empty() ? (server + ":" + port) : remark;
    node.raw_unit = link;
    node.is_link_unit = true;
    nodes.push_back(std::move(node));
}

// Parse Clash YAML proxies. Each proxy node is re-serialized verbatim as its
// own YAML block (raw_unit); we read only name/server/port.
static int loadClash(const std::string &content, const std::string &custom_port,
                     std::vector<nodeInfo> &nodes)
{
    Node yaml = Load(content);
    Node proxies;
    if(yaml["proxies"].IsDefined())
        proxies = yaml["proxies"];
    else if(yaml["Proxy"].IsDefined())
        proxies = yaml["Proxy"];
    if(!proxies.IsDefined() || !proxies.IsSequence())
        return 0;

    int added = 0;
    for(size_t i = 0; i < proxies.size(); ++i)
    {
        const Node &p = proxies[i];
        if(!p.IsMap())
            continue;
        nodeInfo node;
        node.remarks = safe_as<std::string>(p["name"]);
        node.server = safe_as<std::string>(p["server"]);
        std::string port = safe_as<std::string>(p["port"]);
        if(!custom_port.empty())
            port = custom_port;
        node.port = to_int(port, 0);
        if(node.remarks.empty())
            node.remarks = node.server + ":" + port;

        // Re-emit this single proxy as a one-element sequence block. Port
        // override is applied by rewriting the node's port key in the emitter.
        Node single = Clone(p);
        if(!custom_port.empty())
            single["port"] = to_int(custom_port, node.port);
        Emitter em;
        em << single;
        node.raw_unit = std::string(em.c_str());
        node.is_link_unit = false;
        nodes.push_back(std::move(node));
        ++added;
    }
    return added;
}

// ---------------------------------------------------------------------------
// Public: loadSubscription
// ---------------------------------------------------------------------------
int loadSubscription(const std::string &content, const std::string &custom_port,
                     std::vector<nodeInfo> &nodes)
{
    std::string body = trim(content);
    if(body.empty())
        return 0;

    // 1) Single share link (the whole input is one link).
    if(isShareLink(body) && body.find('\n') == body.npos &&
       body.find('\r') == body.npos)
    {
        addLinkNode(body, custom_port, nodes);
        return 1;
    }

    // 2) Clash YAML (has a proxies/Proxy list).
    if(regFind(body, "(?:^|\\n)\\s*(?:proxies|Proxy)\\s*:"))
    {
        int n = loadClash(body, custom_port, nodes);
        if(n > 0)
            return n;
    }

    // 3) base64 share-link list (decode, then one link per line).
    std::string decoded = urlsafe_base64_decode(body);
    const std::string &lines = isShareLink(trim(decoded)) ? decoded : body;
    std::stringstream ss(lines);
    std::string line;
    int added = 0;
    while(std::getline(ss, line))
    {
        line = trim(line);
        if(line.empty())
            continue;
        if(line.rfind('\r') != line.npos)
            line.erase(line.find('\r'));
        if(isShareLink(line))
        {
            addLinkNode(line, custom_port, nodes);
            ++added;
        }
    }
    return added;
}

// ---------------------------------------------------------------------------
// Public: buildProvidersConfig
// ---------------------------------------------------------------------------
ProvidersBuild buildProvidersConfig(std::vector<nodeInfo> &nodes,
                                    int socks_port,
                                    const std::string &controller)
{
    ProvidersBuild out;

    // Split surviving nodes into a YAML-proxy unit and a share-link unit,
    // preserving order so the kernel's /providers/proxies response zips back.
    Node yaml_proxies(NodeType::Sequence);
    std::string link_list;
    for(auto &n : nodes)
    {
        if(n.proxyStr == "LOG" || n.raw_unit.empty())
            continue;
        if(n.is_link_unit)
        {
            link_list += n.raw_unit + "\n";
            out.link_order.push_back(&n);
        }
        else
        {
            yaml_proxies.push_back(Load(n.raw_unit));
            out.yaml_order.push_back(&n);
        }
    }

    std::string providers, groups_use;
    if(!out.yaml_order.empty())
    {
        Node root(NodeType::Map);
        root["proxies"] = yaml_proxies;
        Emitter em;
        em << root;
        out.yaml_provider_path = "sub_yaml.yaml";
        out.yaml_provider_body = std::string(em.c_str());
        providers += "  yaml_sub:\n    type: file\n    path: ./sub_yaml.yaml\n"
                     "    health-check:\n      enable: false\n";
        groups_use += "      - yaml_sub\n";
    }
    if(!out.link_order.empty())
    {
        // base64 share-link list — proven to load directly via a file provider.
        out.link_provider_path = "sub_link.txt";
        out.link_provider_body = base64_encode(link_list);
        providers += "  link_sub:\n    type: file\n    path: ./sub_link.txt\n"
                     "    health-check:\n      enable: false\n";
        groups_use += "      - link_sub\n";
    }

    std::string cfg;
    cfg += "mixed-port: 0\n";
    cfg += "socks-port: " + std::to_string(socks_port) + "\n";
    cfg += "allow-lan: false\n";
    cfg += "bind-address: '127.0.0.1'\n";
    cfg += "mode: global\n";
    cfg += "log-level: warning\n";
    cfg += "ipv6: true\n";
    cfg += "external-controller: '" + controller + "'\n";
    cfg += "secret: ''\n";
    if(!providers.empty())
    {
        cfg += "proxy-providers:\n" + providers;
        cfg += "proxy-groups:\n  - name: GLOBAL\n    type: select\n    use:\n" + groups_use;
    }
    out.config_yaml = cfg;
    return out;
}
