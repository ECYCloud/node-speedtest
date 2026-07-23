use std::collections::HashSet;
use std::time::Instant;

use serde::{Deserialize, Serialize};

/// 未填分组时的统一分组名（结果表 / PNG 标题 / 历史文件名前缀）。
pub const DEFAULT_GROUP_LABEL: &str = "Node Speedtest";

/// 导出/标题用分组：自定义优先；未填一律 Node Speedtest（不回退成协议名）。
pub fn resolve_test_group(custom_group: &str, _nodes: &[Node]) -> String {
    let custom = custom_group.trim();
    if !custom.is_empty() {
        return custom.to_string();
    }
    DEFAULT_GROUP_LABEL.into()
}

#[derive(Debug, Clone, Default)]
pub struct EngineState {
    pub running: bool,
    pub ping_only: bool,
    pub sort_method: String,
    pub custom_group: String,
    pub all_nodes: Vec<Node>,
    pub target_nodes: Vec<Node>,
    pub current_id: i32,
    pub completed_ids: HashSet<i32>,
    pub done_at: Option<Instant>,
    pub mihomo_version: String,
    pub socks_port: u16,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub id: i32,
    pub group: String,
    pub remarks: String,
    pub server: String,
    pub port: i32,
    pub proxy_str: String,
    pub proxy_type: String,
    pub raw_unit: String,
    pub is_link_unit: bool,
    pub online: bool,
    pub avg_ping_ms: f64,
    pub site_ping_ms: f64,
    pub pk_loss: String,
    pub avg_speed: String,
    pub max_speed: String,
    pub ul_speed: String,
    pub raw_speed: [u64; 20],
    pub raw_ping: [i32; 6],
    pub raw_site_ping: [i32; 10],
    pub total_recv_bytes: u64,
    pub test_file: String,
    pub nat_type: String,
    /// NotApplicable / Verified / Failed
    pub tls_verified: String,
    pub inbound_geo: GeoBlock,
    pub outbound_geo: GeoBlock,
}

impl Default for Node {
    fn default() -> Self {
        Self {
            id: -1,
            group: String::new(),
            remarks: String::new(),
            server: String::new(),
            port: 0,
            proxy_str: String::new(),
            proxy_type: String::new(),
            raw_unit: String::new(),
            is_link_unit: false,
            online: false,
            avg_ping_ms: 0.0,
            site_ping_ms: 0.0,
            pk_loss: "100.00%".into(),
            avg_speed: "N/A".into(),
            max_speed: "N/A".into(),
            ul_speed: "N/A".into(),
            raw_speed: [0; 20],
            raw_ping: [0; 6],
            raw_site_ping: [0; 10],
            total_recv_bytes: 0,
            test_file: DEFAULT_TEST_FILE.into(),
            nat_type: "Unknown".into(),
            tls_verified: "NotApplicable".into(),
            inbound_geo: GeoBlock::default(),
            outbound_geo: GeoBlock::default(),
        }
    }
}

pub const DEFAULT_TEST_FILE: &str = "https://speed.cloudflare.com/__down?bytes=95000000";
pub const LATENCY_URL: &str = "https://cp.cloudflare.com/generate_204";
pub const CONTROLLER: &str = "127.0.0.1:9990";
pub const DEFAULT_SOCKS_PORT: u16 = 65432;

#[derive(Debug, Clone, Default, Serialize)]
pub struct GeoBlock {
    pub address: String,
    pub info: String,
    #[serde(skip)]
    pub ip: String,
    #[serde(skip)]
    pub country: String,
    #[serde(skip)]
    pub city: String,
    #[serde(skip)]
    pub organization: String,
    #[serde(skip)]
    pub country_code: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRequest {
    pub configs: Vec<NodeConfigIn>,
    pub test_mode: String,
    pub sort_method: String,
    #[serde(default)]
    pub group: String,
}

#[derive(Debug, Deserialize)]
pub struct NodeConfigIn {
    #[serde(default)]
    pub r#type: String,
    pub config: NodeConfigFields,
}

#[derive(Debug, Deserialize)]
pub struct NodeConfigFields {
    #[serde(default)]
    pub group: String,
    pub remarks: String,
    pub server: String,
    pub server_port: i32,
}

pub fn serialize_web_configs(nodes: &[Node]) -> String {
    let arr: Vec<serde_json::Value> = nodes
        .iter()
        .map(|n| {
            serde_json::json!({
                "type": if n.proxy_type.is_empty() { "Unknown" } else { &n.proxy_type },
                "config": {
                    "group": n.group,
                    "remarks": n.remarks,
                    "server_port": n.port,
                    "server": n.server,
                }
            })
        })
        .collect();
    serde_json::to_string(&arr).unwrap_or_else(|_| "[]".into())
}

pub fn serialize_results(st: &EngineState) -> String {
    let running = st.running;
    let mut current = serde_json::Map::new();
    if running {
        if let Some(n) = st.target_nodes.iter().find(|x| x.id == st.current_id && x.id >= 0) {
            current = node_to_json_map(n);
        }
    }

    let mut results = Vec::new();
    for n in &st.target_nodes {
        if n.id < 0 {
            continue;
        }
        // running：已完成或序号已越过的节点；stopped：仅 completed_ids
        // （停止后不得把未测节点标 done，否则前端会误判「全部测完」）
        let done = if running {
            st.completed_ids.contains(&n.id) || n.id < st.current_id
        } else {
            st.completed_ids.contains(&n.id)
        };
        if done {
            results.push(serde_json::Value::Object(node_to_json_map(n)));
        }
    }

    serde_json::json!({
        "status": if running { "running" } else { "stopped" },
        "current": current,
        "results": results,
        // 本轮目标节点数，供前端进度/完成态在重启后仍可对齐
        "targetCount": st.target_nodes.len(),
    })
    .to_string()
}

fn node_to_json_map(n: &Node) -> serde_json::Map<String, serde_json::Value> {
    let loss_pct = parse_loss_pct(&n.pk_loss);
    let mut m = serde_json::Map::new();
    m.insert("group".into(), json_str(&n.group));
    m.insert("remarks".into(), json_str(&n.remarks));
    m.insert("loss".into(), serde_json::json!(loss_pct / 100.0));
    m.insert("ping".into(), serde_json::json!(n.avg_ping_ms / 1000.0));
    m.insert("gPing".into(), serde_json::json!(n.site_ping_ms / 1000.0));
    m.insert(
        "rawSocketSpeed".into(),
        serde_json::json!(n.raw_speed.iter().map(|&x| x as i64).collect::<Vec<_>>()),
    );
    m.insert(
        "rawTcpPingStatus".into(),
        serde_json::json!(n.raw_ping.iter().map(|&x| x as f64 / 1000.0).collect::<Vec<_>>()),
    );
    let mut g_fail = 0usize;
    let mut g_total = 0usize;
    let g_arr: Vec<f64> = n
        .raw_site_ping
        .iter()
        .map(|&x| {
            g_total += 1;
            if x == 0 {
                g_fail += 1;
            }
            x as f64 / 1000.0
        })
        .collect();
    m.insert("rawGooglePingStatus".into(), serde_json::json!(g_arr));
    m.insert(
        "gPingLoss".into(),
        serde_json::json!(if g_total > 0 {
            g_fail as f64 / g_total as f64
        } else {
            0.0
        }),
    );
    m.insert("webPageSimulation".into(), json_str("N/A"));
    m.insert(
        "geoIP".into(),
        serde_json::json!({
            "inbound": {
                "address": format!("{}:{}", n.server, n.port),
                "info": n.inbound_geo.info,
            },
            "outbound": {
                "address": n.outbound_geo.address,
                "info": n.outbound_geo.info,
            }
        }),
    );
    m.insert("dspeed".into(), serde_json::json!(stream_to_bps(&n.avg_speed)));
    m.insert(
        "dspeedMax".into(),
        serde_json::json!(stream_to_bps(&n.max_speed)),
    );
    m.insert(
        "trafficUsed".into(),
        serde_json::json!(n.total_recv_bytes as i64),
    );
    m.insert("natType".into(), json_str(&n.nat_type));
    m.insert("tlsVerified".into(), json_str(&n.tls_verified));
    m
}

fn json_str(s: &str) -> serde_json::Value {
    serde_json::Value::String(s.to_string())
}

fn parse_loss_pct(s: &str) -> f64 {
    let t = s.trim().trim_end_matches('%');
    t.parse().unwrap_or(0.0)
}

/// 与 C++ streamToInt 对齐：把 "12.34MB" 转为字节/秒数值。
pub fn stream_to_bps(stream: &str) -> f64 {
    if stream.is_empty() || stream == "N/A" {
        return 0.0;
    }
    let units = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];
    for (i, u) in units.iter().enumerate().rev() {
        if stream.ends_with(u) {
            let num: f64 = stream[..stream.len() - u.len()].parse().unwrap_or(0.0);
            return num * 1024f64.powi(i as i32);
        }
    }
    0.0
}

pub fn speed_calc(speed: f64) -> String {
    if speed == 0.0 {
        return "0.00B".into();
    }
    if speed >= 1_073_741_824.0 {
        format!("{:.2}GB", speed / 1_073_741_824.0)
    } else if speed >= 1_048_576.0 {
        format!("{:.2}MB", speed / 1_048_576.0)
    } else if speed >= 1024.0 {
        format!("{:.2}KB", speed / 1024.0)
    } else {
        format!("{:.2}B", speed)
    }
}
