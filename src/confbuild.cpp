#include <string>
#include <vector>
#include <numeric>

#include "ini_reader.h"
#include "misc.h"
#include "nodeinfo.h"

extern int socksport;

// =============================================================================
// mihomo (Clash.Meta) single-node YAML config templates.
//
// Each *Construct() returns a complete mihomo config file content with exactly
// one outbound named "node", consumed by `mihomo -d . -f config.yaml`.
// Legacy SS / SSR / VMess / Snell remain only as empty stubs so the rest of
// the codebase still links; they should never be reached because explodeClash
// only emits VLESS / Trojan / Hysteria2 / AnyTLS nodes.
// =============================================================================

// YAML header shared by every generated config. socksport is substituted in.
static std::string mihomoHeader()
{
    return std::string()
        + "mixed-port: 0\n"
        + "socks-port: " + std::to_string(socksport) + "\n"
        + "allow-lan: false\n"
        + "bind-address: '127.0.0.1'\n"
        + "mode: global\n"
        + "log-level: silent\n"
        + "ipv6: true\n"
        + "external-controller: '127.0.0.1:9990'\n"
        + "secret: ''\n"
        + "geodata-mode: false\n"
        + "geo-auto-update: false\n"
        + "profile:\n"
        + "  store-selected: false\n"
        + "  store-fake-ip: false\n"
        + "dns:\n"
        + "  enable: true\n"
        + "  ipv6: true\n"
        + "  enhanced-mode: redir-host\n"
        + "  default-nameserver: [119.29.29.29, 223.5.5.5]\n"
        + "  nameserver: [119.29.29.29, 223.5.5.5]\n"
        + "  proxy-server-nameserver: [119.29.29.29, 223.5.5.5]\n"
        + "proxies:\n";
}

// proxy-groups + tail. The selector is named GLOBAL; in mode: global mihomo
// routes every connection through the proxy chosen here.
static std::string mihomoTail()
{
    return std::string()
        + "proxy-groups:\n"
        + "  - {name: GLOBAL, type: select, proxies: [node]}\n";
}

// YAML-escape a scalar that will appear inside single quotes.
static std::string yamlEscape(const std::string &s)
{
    std::string out;
    out.reserve(s.size() + 8);
    for(char c : s)
    {
        if(c == '\'')
            out += "''";
        else
            out += c;
    }
    return out;
}

// Build a quoted scalar; if input is empty return empty quoted string.
static std::string yq(const std::string &s) { return "'" + yamlEscape(s) + "'"; }

// Common ECH block. Uses query-server-name semantic from user's subscription.
static std::string echBlock(const std::string &ech_server_name, const std::string &indent)
{
    if(ech_server_name.empty())
        return std::string();
    return indent + "ech-opts:\n"
         + indent + "  enable: true\n"
         + indent + "  query-server-name: " + yq(ech_server_name) + "\n";
}


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

std::string replace_first(std::string str, const std::string &old_value, const std::string &new_value)
{
    string_size pos = str.find(old_value);
    if(pos == str.npos)
        return str;
    return str.replace(pos, old_value.size(), new_value);
}

std::string vmessConstruct(const std::string &group, const std::string &remarks, const std::string &add, const std::string &port, const std::string &type, const std::string &id, const std::string &aid, const std::string &net, const std::string &cipher, const std::string &path, const std::string &host, const std::string &edge, const std::string &tls, tribool udp, tribool tfo, tribool scv, tribool tls13)
{
    // VMess support intentionally disabled in mihomo-only build.
    (void)group; (void)remarks; (void)add; (void)port; (void)type;
    (void)id; (void)aid; (void)net; (void)cipher; (void)path;
    (void)host; (void)edge; (void)tls; (void)udp; (void)tfo;
    (void)scv; (void)tls13;
    return std::string();
}

std::string ssrConstruct(const std::string &group, const std::string &remarks, const std::string &remarks_base64, const std::string &server, const std::string &port, const std::string &protocol, const std::string &method, const std::string &obfs, const std::string &password, const std::string &obfsparam, const std::string &protoparam, bool libev, tribool udp, tribool tfo, tribool scv)
{
    // ShadowsocksR intentionally disabled in mihomo-only build.
    (void)group; (void)remarks; (void)remarks_base64; (void)server; (void)port;
    (void)protocol; (void)method; (void)obfs; (void)password; (void)obfsparam;
    (void)protoparam; (void)libev; (void)udp; (void)tfo; (void)scv;
    return std::string();
}

std::string ssConstruct(const std::string &group, const std::string &remarks, const std::string &server, const std::string &port, const std::string &password, const std::string &method, const std::string &plugin, const std::string &pluginopts, bool libev, tribool udp, tribool tfo, tribool scv, tribool tls13)
{
    // Shadowsocks intentionally disabled in mihomo-only build.
    (void)group; (void)remarks; (void)server; (void)port; (void)password;
    (void)method; (void)plugin; (void)pluginopts; (void)libev; (void)udp;
    (void)tfo; (void)scv; (void)tls13;
    return std::string();
}

std::string socksConstruct(const std::string &group, const std::string &remarks, const std::string &server, const std::string &port, const std::string &username, const std::string &password, tribool udp, tribool tfo, tribool scv)
{
    // SOCKS5 nodes connect to upstream directly without a local kernel.
    (void)group; (void)remarks; (void)server; (void)port;
    (void)udp; (void)tfo; (void)scv;
    return "user=" + username + "&pass=" + password;
}

std::string httpConstruct(const std::string &group, const std::string &remarks, const std::string &server, const std::string &port, const std::string &username, const std::string &password, bool tls, tribool tfo, tribool scv, tribool tls13)
{
    // HTTP/HTTPS proxy nodes connect upstream directly without a local kernel.
    (void)group; (void)remarks; (void)server; (void)port; (void)tls;
    (void)tfo; (void)scv; (void)tls13;
    return "user=" + username + "&pass=" + password;
}

std::string trojanConstruct(const std::string &group, const std::string &remarks, const std::string &server, const std::string &port, const std::string &password, const std::string &host, bool tlssecure, tribool udp, tribool tfo, tribool scv, tribool tls13, const std::string &network, const std::string &ws_path, const std::string &ws_host, const std::string &ech_server_name)
{
    (void)group; (void)remarks; (void)tfo; (void)tls13;
    std::string out = mihomoHeader();
    out += "  - name: node\n";
    out += "    type: trojan\n";
    out += "    server: " + yq(server) + "\n";
    out += "    port: " + port + "\n";
    out += "    password: " + yq(password) + "\n";
    if(!host.empty())
        out += "    sni: " + yq(host) + "\n";
    out += std::string("    udp: ") + (udp.is_undef() ? "true" : (udp ? "true" : "false")) + "\n";
    out += std::string("    skip-cert-verify: ") + (scv.is_undef() ? "false" : (scv ? "true" : "false")) + "\n";
    if(!tlssecure)
        out += "    tls: false\n";
    if(!network.empty() && network != "tcp")
    {
        out += "    network: " + network + "\n";
        if(network == "ws")
        {
            out += "    ws-opts:\n";
            out += "      path: " + yq(ws_path.empty() ? std::string("/") : ws_path) + "\n";
            if(!ws_host.empty())
            {
                out += "      headers:\n";
                out += "        Host: " + yq(ws_host) + "\n";
            }
        }
        else if(network == "grpc")
        {
            out += "    grpc-opts:\n";
            out += "      grpc-service-name: " + yq(ws_path) + "\n";
        }
    }
    out += echBlock(ech_server_name, "    ");
    out += mihomoTail();
    return out;
}

std::string snellConstruct(const std::string &group, const std::string &remarks, const std::string &server, const std::string &port, const std::string &password, const std::string &obfs, const std::string &host, tribool udp, tribool tfo, tribool scv)
{
    // Snell is no longer supported in mihomo-only build.
    (void)group; (void)remarks; (void)server; (void)port; (void)password;
    (void)obfs; (void)host; (void)udp; (void)tfo; (void)scv;
    return std::string();
}

// -----------------------------------------------------------------------------
// New protocols supported by the mihomo kernel.
// -----------------------------------------------------------------------------

std::string vlessConstruct(const std::string &group, const std::string &remarks, const std::string &add, const std::string &port, const std::string &uuid, const std::string &flow, const std::string &encryption, const std::string &network, const std::string &security, const std::string &sni, const std::string &path, const std::string &host, const std::string &ech_server_name, tribool udp, tribool tfo, tribool scv)
{
    (void)group; (void)remarks; (void)encryption; (void)tfo;
    std::string net = network.empty() ? std::string("tcp") : network;
    bool useTLS = (security == "tls" || security == "reality" || security == "xtls");

    std::string out = mihomoHeader();
    out += "  - name: node\n";
    out += "    type: vless\n";
    out += "    server: " + yq(add) + "\n";
    out += "    port: " + port + "\n";
    out += "    uuid: " + yq(uuid) + "\n";
    out += std::string("    udp: ") + (udp.is_undef() ? "true" : (udp ? "true" : "false")) + "\n";
    if(!flow.empty())
        out += "    flow: " + yq(flow) + "\n";
    out += std::string("    tls: ") + (useTLS ? "true" : "false") + "\n";
    if(!sni.empty())
        out += "    servername: " + yq(sni) + "\n";
    out += std::string("    skip-cert-verify: ") + (scv.is_undef() ? "false" : (scv ? "true" : "false")) + "\n";
    out += "    network: " + net + "\n";
    if(net == "ws")
    {
        out += "    ws-opts:\n";
        out += "      path: " + yq(path.empty() ? std::string("/") : path) + "\n";
        if(!host.empty())
        {
            out += "      headers:\n";
            out += "        Host: " + yq(host) + "\n";
        }
    }
    else if(net == "grpc")
    {
        out += "    grpc-opts:\n";
        out += "      grpc-service-name: " + yq(path) + "\n";
    }
    out += echBlock(ech_server_name, "    ");
    out += mihomoTail();
    return out;
}

std::string hysteria2Construct(const std::string &group, const std::string &remarks, const std::string &add, const std::string &port, const std::string &ports, const std::string &password, const std::string &sni, const std::string &obfs, const std::string &obfsPassword, const std::string &up, const std::string &down, const std::string &ech_server_name, tribool udp, tribool tfo, tribool scv)
{
    (void)group; (void)remarks; (void)tfo;
    std::string out = mihomoHeader();
    out += "  - name: node\n";
    out += "    type: hysteria2\n";
    out += "    server: " + yq(add) + "\n";
    out += "    port: " + port + "\n";
    if(!ports.empty())
        out += "    ports: " + yq(ports) + "\n";
    out += "    password: " + yq(password) + "\n";
    if(!sni.empty())
        out += "    sni: " + yq(sni) + "\n";
    if(!obfs.empty())
    {
        out += "    obfs: " + yq(obfs) + "\n";
        if(!obfsPassword.empty())
            out += "    obfs-password: " + yq(obfsPassword) + "\n";
    }
    if(!up.empty())
        out += "    up: " + yq(up) + "\n";
    if(!down.empty())
        out += "    down: " + yq(down) + "\n";
    out += std::string("    udp: ") + (udp.is_undef() ? "true" : (udp ? "true" : "false")) + "\n";
    out += std::string("    skip-cert-verify: ") + (scv.is_undef() ? "false" : (scv ? "true" : "false")) + "\n";
    out += echBlock(ech_server_name, "    ");
    out += mihomoTail();
    return out;
}

std::string anytlsConstruct(const std::string &group, const std::string &remarks, const std::string &add, const std::string &port, const std::string &password, const std::string &sni, const std::string &ech_server_name, tribool udp, tribool tfo, tribool scv)
{
    (void)group; (void)remarks; (void)tfo;
    std::string out = mihomoHeader();
    out += "  - name: node\n";
    out += "    type: anytls\n";
    out += "    server: " + yq(add) + "\n";
    out += "    port: " + port + "\n";
    out += "    password: " + yq(password) + "\n";
    out += "    client-fingerprint: chrome\n";
    if(!sni.empty())
        out += "    sni: " + yq(sni) + "\n";
    out += std::string("    udp: ") + (udp.is_undef() ? "true" : (udp ? "true" : "false")) + "\n";
    out += std::string("    skip-cert-verify: ") + (scv.is_undef() ? "false" : (scv ? "true" : "false")) + "\n";
    out += echBlock(ech_server_name, "    ");
    out += mihomoTail();
    return out;
}
