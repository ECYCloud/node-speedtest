#include <string>
#include <vector>
#include <numeric>

#include "ini_reader.h"
#include "misc.h"
#include "nodeinfo.h"
#include "printout.h"

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
    (void)group; (void)remarks; (void)type; (void)edge; (void)tfo; (void)tls13;
    bool useTLS = (tls == "tls");
    std::string net2 = net.empty() ? std::string("tcp") : net;

    std::string out = mihomoHeader();
    out += "  - name: node\n";
    out += "    type: vmess\n";
    out += "    server: " + yq(add) + "\n";
    out += "    port: " + port + "\n";
    out += "    uuid: " + yq(id) + "\n";
    out += "    alterId: " + (aid.empty() ? std::string("0") : aid) + "\n";
    out += "    cipher: " + (cipher.empty() ? std::string("auto") : cipher) + "\n";
    out += std::string("    udp: ") + (udp.is_undef() ? "true" : (udp ? "true" : "false")) + "\n";
    out += std::string("    tls: ") + (useTLS ? "true" : "false") + "\n";
    out += std::string("    skip-cert-verify: ") + (scv.is_undef() ? "false" : (scv ? "true" : "false")) + "\n";
    if(useTLS && !host.empty())
        out += "    servername: " + yq(host) + "\n";
    out += "    network: " + net2 + "\n";
    if(net2 == "ws")
    {
        out += "    ws-opts:\n";
        out += "      path: " + yq(path.empty() ? std::string("/") : path) + "\n";
        if(!host.empty())
        {
            out += "      headers:\n";
            out += "        Host: " + yq(host) + "\n";
        }
    }
    else if(net2 == "grpc")
    {
        out += "    grpc-opts:\n";
        out += "      grpc-service-name: " + yq(path) + "\n";
    }
    out += mihomoTail();
    return out;
}

std::string ssrConstruct(const std::string &group, const std::string &remarks, const std::string &remarks_base64, const std::string &server, const std::string &port, const std::string &protocol, const std::string &method, const std::string &obfs, const std::string &password, const std::string &obfsparam, const std::string &protoparam, bool libev, tribool udp, tribool tfo, tribool scv)
{
    (void)group; (void)remarks; (void)remarks_base64; (void)libev; (void)tfo; (void)scv;
    std::string out = mihomoHeader();
    out += "  - name: node\n";
    out += "    type: ssr\n";
    out += "    server: " + yq(server) + "\n";
    out += "    port: " + port + "\n";
    out += "    cipher: " + yq(method) + "\n";
    out += "    password: " + yq(password) + "\n";
    out += "    protocol: " + yq(protocol) + "\n";
    out += "    obfs: " + yq(obfs) + "\n";
    out += std::string("    udp: ") + (udp.is_undef() ? "true" : (udp ? "true" : "false")) + "\n";
    if(!protoparam.empty())
        out += "    protocol-param: " + yq(protoparam) + "\n";
    if(!obfsparam.empty())
        out += "    obfs-param: " + yq(obfsparam) + "\n";
    out += mihomoTail();
    return out;
}

std::string ssConstruct(const std::string &group, const std::string &remarks, const std::string &server, const std::string &port, const std::string &password, const std::string &method, const std::string &plugin, const std::string &pluginopts, bool libev, tribool udp, tribool tfo, tribool scv, tribool tls13)
{
    (void)group; (void)remarks; (void)libev; (void)tfo; (void)scv; (void)tls13;
    std::string out = mihomoHeader();
    out += "  - name: node\n";
    out += "    type: ss\n";
    out += "    server: " + yq(server) + "\n";
    out += "    port: " + port + "\n";
    out += "    cipher: " + yq(method) + "\n";
    out += "    password: " + yq(password) + "\n";
    out += std::string("    udp: ") + (udp.is_undef() ? "true" : (udp ? "true" : "false")) + "\n";

    if(!plugin.empty())
    {
        // Parse pluginopts like "obfs=http;obfs-host=abc.com" into key/value pairs.
        std::string obfsMode, obfsHost, wsHost, wsPath;
        bool wsTLS = false;
        std::vector<std::string> opts = split(pluginopts, ";");
        for(auto &kv : opts)
        {
            std::string::size_type eq = kv.find('=');
            if(eq == std::string::npos)
            {
                if(kv == "tls")
                    wsTLS = true;
                continue;
            }
            std::string key = kv.substr(0, eq);
            std::string val = kv.substr(eq + 1);
            if(key == "obfs")
                obfsMode = val;
            else if(key == "obfs-host")
                obfsHost = val;
            else if(key == "host")
                wsHost = val;
            else if(key == "path")
                wsPath = val;
            else if(key == "tls")
                wsTLS = true;
        }

        if(plugin == "obfs" || plugin == "simple-obfs" || plugin == "obfs-local")
        {
            out += "    plugin: obfs\n";
            out += "    plugin-opts:\n";
            out += "      mode: " + (obfsMode.empty() ? std::string("http") : obfsMode) + "\n";
            if(!obfsHost.empty())
                out += "      host: " + yq(obfsHost) + "\n";
        }
        else if(plugin == "v2ray-plugin")
        {
            out += "    plugin: v2ray-plugin\n";
            out += "    plugin-opts:\n";
            out += "      mode: websocket\n";
            if(!wsHost.empty())
                out += "      host: " + yq(wsHost) + "\n";
            if(!wsPath.empty())
                out += "      path: " + yq(wsPath) + "\n";
            if(wsTLS)
                out += "      tls: true\n";
        }
        // Any other plugin value: emit no plugin block.
    }
    out += mihomoTail();
    return out;
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
    (void)group; (void)remarks; (void)tfo; (void)scv;
    std::string out = mihomoHeader();
    out += "  - name: node\n";
    out += "    type: snell\n";
    out += "    server: " + yq(server) + "\n";
    out += "    port: " + port + "\n";
    out += "    psk: " + yq(password) + "\n";
    out += "    version: 2\n";
    out += std::string("    udp: ") + (udp.is_undef() ? "true" : (udp ? "true" : "false")) + "\n";
    if(!obfs.empty())
    {
        out += "    obfs-opts:\n";
        out += "      mode: " + obfs + "\n";
        if(!host.empty())
            out += "      host: " + yq(host) + "\n";
    }
    out += mihomoTail();
    return out;
}

// -----------------------------------------------------------------------------
// New protocols supported by the mihomo kernel.
// -----------------------------------------------------------------------------

std::string vlessConstruct(const std::string &group, const std::string &remarks, const std::string &add, const std::string &port, const std::string &uuid, const std::string &flow, const std::string &encryption, const std::string &network, const std::string &security, const std::string &sni, const std::string &path, const std::string &host, const std::string &ech_server_name, tribool udp, tribool tfo, tribool scv, const std::string &reality_pbk, const std::string &reality_sid, const std::string &client_fingerprint)
{
    (void)group; (void)remarks; (void)encryption; (void)tfo;
    std::string net = network.empty() ? std::string("tcp") : network;
    bool useReality = (security == "reality") && !reality_pbk.empty();
    bool useTLS = useReality || (security == "tls" || security == "xtls");

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
    // Reality(基于 TLS 的反审查传输):必须同时给出 public-key,short-id 可选。
    // mihomo 还需要 client-fingerprint，缺省时给 chrome，与主流 Reality 客户端一致。
    if(useReality)
    {
        out += "    client-fingerprint: " + (client_fingerprint.empty() ? std::string("chrome") : client_fingerprint) + "\n";
        out += "    reality-opts:\n";
        out += "      public-key: " + yq(reality_pbk) + "\n";
        if(!reality_sid.empty())
            out += "      short-id: " + yq(reality_sid) + "\n";
    }
    else if(!client_fingerprint.empty())
    {
        // 非 Reality 也允许指定 fingerprint(uTLS 伪装)
        out += "    client-fingerprint: " + client_fingerprint + "\n";
    }
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

std::string hysteria2Construct(const std::string &group, const std::string &remarks, const std::string &add, const std::string &port, const std::string &ports, const std::string &password, const std::string &sni, const std::string &obfs, const std::string &obfsPassword, const std::string &up, const std::string &down, const std::string &ech_server_name, tribool udp, tribool tfo, tribool scv, const std::string &hop_interval)
{
    (void)group; (void)remarks; (void)tfo;
    std::string out = mihomoHeader();
    out += "  - name: node\n";
    out += "    type: hysteria2\n";
    out += "    server: " + yq(add) + "\n";
    out += "    port: " + port + "\n";
    if(!ports.empty())
        out += "    ports: " + yq(ports) + "\n";
    if(!hop_interval.empty())
        out += "    hop-interval: " + hop_interval + "\n";
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

// TUIC v5(uuid+password)/ v4(token)。mihomo 字段:
//   type: tuic / server / port / uuid / password (v5) 或 token (v4) /
//   sni / alpn / congestion-controller / udp-relay-mode / reduce-rtt
std::string tuicConstruct(const std::string &group, const std::string &remarks, const std::string &add, const std::string &port, const std::string &uuid, const std::string &password, const std::string &token, const std::string &sni, const std::string &alpn, const std::string &congestion, const std::string &udp_relay_mode, bool reduce_rtt, const std::string &ech_server_name, tribool udp, tribool tfo, tribool scv)
{
    (void)group; (void)remarks; (void)tfo; (void)udp;
    std::string out = mihomoHeader();
    out += "  - name: node\n";
    out += "    type: tuic\n";
    out += "    server: " + yq(add) + "\n";
    out += "    port: " + port + "\n";
    if(!token.empty())
        out += "    token: " + yq(token) + "\n";
    if(!uuid.empty())
        out += "    uuid: " + yq(uuid) + "\n";
    if(!password.empty())
        out += "    password: " + yq(password) + "\n";
    if(!sni.empty())
        out += "    sni: " + yq(sni) + "\n";
    if(!alpn.empty())
        out += "    alpn: [" + alpn + "]\n";
    out += "    congestion-controller: " + (congestion.empty() ? std::string("bbr") : congestion) + "\n";
    out += "    udp-relay-mode: " + (udp_relay_mode.empty() ? std::string("native") : udp_relay_mode) + "\n";
    if(reduce_rtt)
        out += "    reduce-rtt: true\n";
    out += std::string("    skip-cert-verify: ") + (scv.is_undef() ? "false" : (scv ? "true" : "false")) + "\n";
    out += echBlock(ech_server_name, "    ");
    out += mihomoTail();
    return out;
}

// Hysteria v1。mihomo 字段:auth-str / up / down / obfs / alpn / sni / protocol / ports
std::string hysteriaConstruct(const std::string &group, const std::string &remarks, const std::string &add, const std::string &port, const std::string &ports, const std::string &auth, const std::string &sni, const std::string &up, const std::string &down, const std::string &obfs, const std::string &alpn, const std::string &protocol, tribool udp, tribool tfo, tribool scv)
{
    (void)group; (void)remarks; (void)tfo; (void)udp;
    std::string out = mihomoHeader();
    out += "  - name: node\n";
    out += "    type: hysteria\n";
    out += "    server: " + yq(add) + "\n";
    out += "    port: " + port + "\n";
    if(!ports.empty())
        out += "    ports: " + yq(ports) + "\n";
    if(!auth.empty())
        out += "    auth-str: " + yq(auth) + "\n";
    if(!up.empty())
        out += "    up: " + yq(up) + "\n";
    if(!down.empty())
        out += "    down: " + yq(down) + "\n";
    if(!obfs.empty())
        out += "    obfs: " + yq(obfs) + "\n";
    if(!sni.empty())
        out += "    sni: " + yq(sni) + "\n";
    if(!alpn.empty())
        out += "    alpn: [" + alpn + "]\n";
    if(!protocol.empty())
        out += "    protocol: " + yq(protocol) + "\n";
    out += std::string("    skip-cert-verify: ") + (scv.is_undef() ? "true" : (scv ? "true" : "false")) + "\n";
    out += mihomoTail();
    return out;
}

// WireGuard。mihomo 字段:private-key / public-key / ip / (ipv6) / pre-shared-key / reserved
std::string wireguardConstruct(const std::string &group, const std::string &remarks, const std::string &add, const std::string &port, const std::string &private_key, const std::string &public_key, const std::string &ip, const std::string &ipv6, const std::string &preshared_key, const std::string &reserved, tribool udp)
{
    (void)group; (void)remarks;
    std::string out = mihomoHeader();
    out += "  - name: node\n";
    out += "    type: wireguard\n";
    out += "    server: " + yq(add) + "\n";
    out += "    port: " + port + "\n";
    out += "    private-key: " + yq(private_key) + "\n";
    out += "    public-key: " + yq(public_key) + "\n";
    if(!ip.empty())
        out += "    ip: " + yq(ip) + "\n";
    if(!ipv6.empty())
        out += "    ipv6: " + yq(ipv6) + "\n";
    if(!preshared_key.empty())
        out += "    pre-shared-key: " + yq(preshared_key) + "\n";
    if(!reserved.empty())
        out += "    reserved: " + reserved + "\n";
    out += std::string("    udp: ") + (udp.is_undef() ? "true" : (udp ? "true" : "false")) + "\n";
    out += mihomoTail();
    return out;
}

// SSH。mihomo 字段:username / password / private-key / host-key
std::string sshConstruct(const std::string &group, const std::string &remarks, const std::string &add, const std::string &port, const std::string &username, const std::string &password, const std::string &private_key)
{
    (void)group; (void)remarks;
    std::string out = mihomoHeader();
    out += "  - name: node\n";
    out += "    type: ssh\n";
    out += "    server: " + yq(add) + "\n";
    out += "    port: " + port + "\n";
    if(!username.empty())
        out += "    username: " + yq(username) + "\n";
    if(!password.empty())
        out += "    password: " + yq(password) + "\n";
    if(!private_key.empty())
        out += "    private-key: " + yq(private_key) + "\n";
    out += mihomoTail();
    return out;
}

// Mieru。mihomo 字段:username / password / transport / port-range
std::string mieruConstruct(const std::string &group, const std::string &remarks, const std::string &add, const std::string &port, const std::string &port_range, const std::string &username, const std::string &password, const std::string &transport, tribool udp)
{
    (void)group; (void)remarks; (void)udp;
    std::string out = mihomoHeader();
    out += "  - name: node\n";
    out += "    type: mieru\n";
    out += "    server: " + yq(add) + "\n";
    if(!port_range.empty())
        out += "    port-range: " + yq(port_range) + "\n";
    else
        out += "    port: " + port + "\n";
    if(!username.empty())
        out += "    username: " + yq(username) + "\n";
    if(!password.empty())
        out += "    password: " + yq(password) + "\n";
    out += "    transport: " + (transport.empty() ? std::string("TCP") : transport) + "\n";
    out += mihomoTail();
    return out;
}

// ShadowTLS:本质是 ss 节点 + shadow-tls 插件。mihomo:type: ss + plugin: shadow-tls
std::string shadowtlsConstruct(const std::string &group, const std::string &remarks, const std::string &add, const std::string &port, const std::string &password, const std::string &method, const std::string &stls_password, const std::string &stls_host, const std::string &version, tribool udp, tribool tfo, tribool scv)
{
    (void)group; (void)remarks; (void)tfo; (void)scv;
    std::string out = mihomoHeader();
    out += "  - name: node\n";
    out += "    type: ss\n";
    out += "    server: " + yq(add) + "\n";
    out += "    port: " + port + "\n";
    out += "    cipher: " + (method.empty() ? std::string("aes-256-gcm") : method) + "\n";
    out += "    password: " + yq(password) + "\n";
    out += "    plugin: shadow-tls\n";
    out += "    plugin-opts:\n";
    out += "      host: " + yq(stls_host) + "\n";
    out += "      password: " + yq(stls_password) + "\n";
    out += "      version: " + (version.empty() ? std::string("3") : version) + "\n";
    out += std::string("    udp: ") + (udp.is_undef() ? "true" : (udp ? "true" : "false")) + "\n";
    out += mihomoTail();
    return out;
}
// kernel exactly once and switch outbounds via Clash API instead of restarting.
//
// `nodes` is mutated: each node.proxyStr (currently a self-contained single
// node YAML) is replaced with the node's name string ("node-<id>"), which the
// rest of the program later passes to mihomoSwitchProxy().
//
// Direct-connect proxies (SOCKS5 / HTTP) keep their original proxyStr
// ("user=...&pass=...") because they don't go through mihomo.
// =============================================================================
std::string buildAllNodesYAML(std::vector<nodeInfo> &nodes)
{
    std::string out = mihomoHeader();
    std::vector<std::string> proxy_names;
    proxy_names.reserve(nodes.size());

    for(auto &node : nodes)
    {
        if(node.linkType == SPEEDTEST_MESSAGE_FOUNDSOCKS ||
           node.linkType == SPEEDTEST_MESSAGE_FOUNDHTTP)
            continue; // direct-connect, no kernel routing
        if(node.proxyStr.empty())
            continue; // legacy stub (ss/ssr/vmess/snell)

        // proxyStr currently looks like:
        //   <header>proxies:\n  - name: node\n    type: vless\n    ...\n<tail>
        // Slice out the proxies entry (between "proxies:\n" and "proxy-groups:")
        const std::string proxies_marker = "proxies:\n";
        const std::string groups_marker = "proxy-groups:";
        std::string::size_type p = node.proxyStr.find(proxies_marker);
        std::string::size_type g = node.proxyStr.find(groups_marker);
        if(p == std::string::npos || g == std::string::npos || g <= p)
            continue;

        std::string entry = node.proxyStr.substr(p + proxies_marker.size(),
                                                  g - p - proxies_marker.size());

        // Rename the inline "node" to a unique id so all entries can coexist
        const std::string old_head = "  - name: node\n";
        if(entry.compare(0, old_head.size(), old_head) != 0)
            continue; // unexpected shape, skip safely
        std::string new_name = "node-" + std::to_string(proxy_names.size());
        entry.replace(0, old_head.size(), "  - name: " + new_name + "\n");

        out += entry;
        proxy_names.push_back(new_name);
        node.proxyStr = new_name; // singleTest will pass this to mihomoSwitchProxy
    }

    // proxy-groups: a single GLOBAL selector listing every node we packed
    out += "proxy-groups:\n";
    out += "  - name: GLOBAL\n";
    out += "    type: select\n";
    out += "    proxies:\n";
    if(proxy_names.empty())
    {
        out += "      - DIRECT\n"; // mihomo refuses an empty selector
    }
    else
    {
        for(const auto &n : proxy_names)
            out += "      - " + n + "\n";
    }
    return out;
}
