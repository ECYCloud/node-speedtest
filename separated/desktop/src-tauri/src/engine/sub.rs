use base64::{engine::general_purpose, Engine as _};
use regex::Regex;

use super::types::Node;

const MAX_SUB_BYTES: u64 = 10 * 1024 * 1024;

pub async fn fetch_subscription(http: &reqwest::Client, url: &str) -> Result<String, String> {
    let ua = format!(
        "NodeSpeedtest/{} Clash mihomo/unknown",
        env!("CARGO_PKG_VERSION")
    );
    let res = http
        .get(url)
        .header("User-Agent", ua)
        .send()
        .await
        .map_err(|e| format!("下载订阅失败: {e}"))?;
    if !res.status().is_success() {
        return Err(format!("下载订阅 HTTP {}", res.status().as_u16()));
    }
    if let Some(len) = res.content_length() {
        if len > MAX_SUB_BYTES {
            return Err(format!("订阅过大({len} 字节，上限 10 MB)"));
        }
    }
    let bytes = res
        .bytes()
        .await
        .map_err(|e| format!("读取订阅正文失败: {e}"))?;
    if bytes.len() as u64 > MAX_SUB_BYTES {
        return Err(format!(
            "订阅过大({} 字节，上限 10 MB)",
            bytes.len()
        ));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub fn load_subscription(content: &str, custom_port: &str) -> Result<Vec<Node>, String> {
    let body = content.trim();
    if body.is_empty() {
        return Ok(Vec::new());
    }

    if is_share_link(body) && !body.contains('\n') && !body.contains('\r') {
        return Ok(vec![link_node(body, custom_port)]);
    }

    let re = Regex::new(r"(?m)^\s*(?:proxies|Proxy)\s*:").unwrap();
    if re.is_match(body) {
        let n = load_clash(body, custom_port)?;
        if !n.is_empty() {
            return Ok(n);
        }
    }

    let decoded = urlsafe_base64_decode(body);
    let lines_src = if is_share_link(decoded.trim()) {
        decoded
    } else {
        body.to_string()
    };
    let mut nodes = Vec::new();
    for line in lines_src.lines() {
        let line = line.trim().trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        if is_share_link(line) {
            nodes.push(link_node(line, custom_port));
        }
    }
    Ok(nodes)
}

fn load_clash(content: &str, custom_port: &str) -> Result<Vec<Node>, String> {
    let yaml: serde_yaml::Value =
        serde_yaml::from_str(content).map_err(|e| format!("YAML 解析失败: {e}"))?;
    let proxies = yaml
        .get("proxies")
        .or_else(|| yaml.get("Proxy"))
        .and_then(|v| v.as_sequence())
        .ok_or_else(|| "无 proxies 列表".to_string())?;

    let mut nodes = Vec::new();
    for p in proxies {
        let map = match p.as_mapping() {
            Some(m) => m,
            None => continue,
        };
        let remarks = yaml_str(map, "name");
        let server = yaml_str(map, "server");
        let mut port = yaml_str(map, "port");
        if !custom_port.is_empty() {
            port = custom_port.to_string();
        }
        let port_i: i32 = port.parse().unwrap_or(0);
        let remarks = if remarks.is_empty() {
            format!("{server}:{port}")
        } else {
            remarks
        };

        let mut single = p.clone();
        if !custom_port.is_empty() {
            if let Some(m) = single.as_mapping_mut() {
                m.insert(
                    serde_yaml::Value::String("port".into()),
                    serde_yaml::Value::Number(port_i.into()),
                );
            }
        }
        let raw_unit = serde_yaml::to_string(&single).unwrap_or_default();
        // serde_yaml 会带文档头 ---，mihomo 单节点块只要 map 文本即可
        let raw_unit = raw_unit
            .trim_start_matches("---")
            .trim()
            .to_string();

        nodes.push(Node {
            group: default_group_for(&remarks),
            remarks,
            server,
            port: port_i,
            raw_unit,
            is_link_unit: false,
            ..Node::default()
        });
    }
    Ok(nodes)
}

fn yaml_str(map: &serde_yaml::Mapping, key: &str) -> String {
    map.get(serde_yaml::Value::String(key.into()))
        .map(|v| match v {
            serde_yaml::Value::String(s) => s.clone(),
            serde_yaml::Value::Number(n) => n.to_string(),
            serde_yaml::Value::Bool(b) => b.to_string(),
            _ => String::new(),
        })
        .unwrap_or_default()
}

fn link_node(link: &str, custom_port: &str) -> Node {
    let (server, mut port, remark) = link_meta(link);
    if !custom_port.is_empty() {
        port = custom_port.to_string();
    }
    let port_i: i32 = port.parse().unwrap_or(0);
    let remarks = if remark.is_empty() {
        format!("{server}:{port}")
    } else {
        remark
    };
    Node {
        group: default_group_for(&remarks),
        remarks,
        server,
        port: port_i,
        raw_unit: link.to_string(),
        is_link_unit: true,
        ..Node::default()
    }
}

fn default_group_for(_remarks: &str) -> String {
    "Default".into()
}

pub fn is_share_link(s: &str) -> bool {
    let Some(p) = s.find("://") else {
        return false;
    };
    if p == 0 {
        return false;
    }
    let scheme = s[..p].to_lowercase();
    if scheme == "http" || scheme == "https" {
        return false;
    }
    scheme
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
}

fn link_meta(link: &str) -> (String, String, String) {
    let mut body = link.to_string();
    if let Some(p) = body.find("://") {
        body = body[p + 3..].to_string();
    }
    let mut remark = String::new();
    if let Some(hash) = body.find('#') {
        remark = urlencoding::decode(&body[hash + 1..])
            .unwrap_or_default()
            .into_owned();
        body = body[..hash].to_string();
    }
    if let Some(q) = body.find('?') {
        body = body[..q].to_string();
    }
    if let Some(at) = body.rfind('@') {
        body = body[at + 1..].to_string();
    }
    if let Some(slash) = body.find('/') {
        body = body[..slash].to_string();
    }

    let mut server = String::new();
    let mut port = String::new();
    if body.starts_with('[') {
        if let Some(rb) = body.find(']') {
            server = body[1..rb].to_string();
            if body.len() > rb + 1 && body.as_bytes()[rb + 1] == b':' {
                port = body[rb + 2..].to_string();
            }
        }
    } else if let Some(colon) = body.rfind(':') {
        if !body.contains(']') {
            server = body[..colon].to_string();
            port = body[colon + 1..].to_string();
        } else {
            server = body;
        }
    } else {
        server = body;
    }
    (server, port, remark)
}

fn urlsafe_base64_decode(s: &str) -> String {
    let mut t = s.trim().replace('-', "+").replace('_', "/");
    while t.len() % 4 != 0 {
        t.push('=');
    }
    general_purpose::STANDARD
        .decode(t.as_bytes())
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .unwrap_or_default()
}

pub fn build_providers_config(
    nodes: &[Node],
    socks_port: u16,
    controller: &str,
) -> ProvidersBuild {
    let mut yaml_proxies: Vec<serde_yaml::Value> = Vec::new();
    let mut link_list = String::new();
    let mut yaml_order = Vec::new();
    let mut link_order = Vec::new();

    for (idx, n) in nodes.iter().enumerate() {
        if n.proxy_str == "LOG" || n.raw_unit.is_empty() {
            continue;
        }
        if n.is_link_unit {
            link_list.push_str(&n.raw_unit);
            link_list.push('\n');
            link_order.push(idx);
        } else if let Ok(v) = serde_yaml::from_str::<serde_yaml::Value>(&n.raw_unit) {
            yaml_proxies.push(v);
            yaml_order.push(idx);
        }
    }

    let mut providers = String::new();
    let mut groups_use = String::new();
    let mut yaml_provider_path = String::new();
    let mut yaml_provider_body = String::new();
    let mut link_provider_path = String::new();
    let mut link_provider_body = String::new();

    if !yaml_order.is_empty() {
        let root = serde_yaml::Mapping::from_iter([(
            serde_yaml::Value::String("proxies".into()),
            serde_yaml::Value::Sequence(yaml_proxies),
        )]);
        yaml_provider_path = "sub_yaml.yaml".into();
        yaml_provider_body =
            serde_yaml::to_string(&serde_yaml::Value::Mapping(root)).unwrap_or_default();
        providers.push_str(
            "  yaml_sub:\n    type: file\n    path: ./sub_yaml.yaml\n    health-check:\n      enable: false\n",
        );
        groups_use.push_str("      - yaml_sub\n");
    }
    if !link_order.is_empty() {
        link_provider_path = "sub_link.txt".into();
        link_provider_body = general_purpose::STANDARD.encode(link_list.as_bytes());
        providers.push_str(
            "  link_sub:\n    type: file\n    path: ./sub_link.txt\n    health-check:\n      enable: false\n",
        );
        groups_use.push_str("      - link_sub\n");
    }

    let mut cfg = String::new();
    cfg.push_str("mixed-port: 0\n");
    cfg.push_str(&format!("socks-port: {socks_port}\n"));
    cfg.push_str("allow-lan: false\n");
    cfg.push_str("bind-address: '127.0.0.1'\n");
    cfg.push_str("mode: global\n");
    cfg.push_str("log-level: warning\n");
    cfg.push_str("ipv6: true\n");
    cfg.push_str(&format!("external-controller: '{controller}'\n"));
    cfg.push_str("secret: ''\n");
    cfg.push_str("unified-delay: true\n");
    if !providers.is_empty() {
        cfg.push_str("proxy-providers:\n");
        cfg.push_str(&providers);
        cfg.push_str("proxy-groups:\n  - name: GLOBAL\n    type: select\n    use:\n");
        cfg.push_str(&groups_use);
    }

    ProvidersBuild {
        config_yaml: cfg,
        yaml_provider_path,
        yaml_provider_body,
        link_provider_path,
        link_provider_body,
        yaml_order,
        link_order,
    }
}

pub struct ProvidersBuild {
    pub config_yaml: String,
    pub yaml_provider_path: String,
    pub yaml_provider_body: String,
    pub link_provider_path: String,
    pub link_provider_body: String,
    pub yaml_order: Vec<usize>,
    pub link_order: Vec<usize>,
}
