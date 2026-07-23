use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

use super::sub::{build_providers_config, ProvidersBuild};
use super::types::{Node, CONTROLLER, DEFAULT_SOCKS_PORT, LATENCY_URL};

static MIHOMO_CHILD: Mutex<Option<Child>> = Mutex::new(None);

pub struct LaunchResult {
    pub nodes: Vec<Node>,
    pub version: String,
    pub socks_port: u16,
}

fn mihomo_exe_name() -> &'static str {
    #[cfg(windows)]
    {
        "mihomo.exe"
    }
    #[cfg(not(windows))]
    {
        "mihomo"
    }
}

pub fn kill_all() {
    if let Ok(mut g) = MIHOMO_CHILD.lock() {
        if let Some(mut c) = g.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = Command::new("taskkill");
        cmd.args(["/F", "/IM", mihomo_exe_name(), "/T"])
            .creation_flags(0x0800_0000)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let _ = cmd.status();
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("pkill")
            .args(["-f", "mihomo"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn spawn_mihomo(work_dir: &Path) -> Result<(), String> {
    let exe = work_dir.join("tools").join("clients").join(mihomo_exe_name());
    if !exe.exists() {
        return Err(format!("mihomo 不存在: {}", exe.display()));
    }
    let mut cmd = Command::new(&exe);
    cmd.args(["-d", ".", "-f", "config.yaml"])
        .current_dir(work_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    let child = cmd.spawn().map_err(|e| format!("启动 mihomo 失败: {e}"))?;
    if let Ok(mut g) = MIHOMO_CHILD.lock() {
        *g = Some(child);
    }
    Ok(())
}

pub async fn wait_ready(http: &reqwest::Client, timeout_ms: u64) -> bool {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);
    while tokio::time::Instant::now() < deadline {
        if let Ok(res) = http
            .get(format!("http://{CONTROLLER}/version"))
            .send()
            .await
        {
            if let Ok(body) = res.text().await {
                if body.contains("\"version\"") {
                    return true;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

pub async fn is_alive(http: &reqwest::Client) -> bool {
    matches!(
        tokio::time::timeout(
            Duration::from_millis(1500),
            http.get(format!("http://{CONTROLLER}/version")).send()
        )
        .await,
        Ok(Ok(r)) if r.status().as_u16() < 500
    )
}

pub async fn ensure_alive(work_dir: &Path, http: &reqwest::Client) -> bool {
    if is_alive(http).await {
        return true;
    }
    eprintln!("[engine] mihomo 不可达，尝试重启");
    kill_all();
    if spawn_mihomo(work_dir).is_err() {
        return false;
    }
    wait_ready(http, 8000).await
}

pub async fn switch_proxy(http: &reqwest::Client, name: &str) -> bool {
    let body = serde_json::json!({ "name": name });
    match http
        .put(format!("http://{CONTROLLER}/proxies/GLOBAL"))
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
    {
        Ok(r) => r.status().is_success(),
        Err(_) => false,
    }
}

pub async fn measure_delay(http: &reqwest::Client, name: &str) -> Option<i32> {
    // timeout 与 C++ mihomoMeasureDelay 默认 8000 对齐。
    // mihomo 1.19.x + proxy-providers：节点常只出现在 provider 里，不在
    // /proxies 顶层 → /proxies/{节点名}/delay 会 404。测速前已 switch 到该节点，
    // 对 GLOBAL 测 delay 才是有效路径（本机实测 ~320ms；节点名路径 404）。
    const TIMEOUT_MS: u32 = 8000;
    let url_q = urlencoding::encode(LATENCY_URL);
    let candidates = [
        format!("http://{CONTROLLER}/proxies/GLOBAL/delay?timeout={TIMEOUT_MS}&url={url_q}"),
        format!(
            "http://{CONTROLLER}/proxies/{}/delay?timeout={TIMEOUT_MS}&url={url_q}",
            urlencoding::encode(name)
        ),
    ];
    for ep in candidates {
        let Ok(res) = http.get(&ep).send().await else {
            continue;
        };
        let Ok(body) = res.text().await else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) else {
            continue;
        };
        if let Some(d) = v.get("delay").and_then(|x| {
            x.as_f64()
                .or_else(|| x.as_i64().map(|n| n as f64))
                .or_else(|| x.as_u64().map(|n| n as f64))
        }) {
            let d = d as i32;
            if d > 0 {
                return Some(d);
            }
        }
    }
    None
}

/// 本地内核版本唯一来源：执行 `mihomo -v`。
/// 输出形如：`Mihomo Meta v1.19.29 windows amd64 ...`
pub fn read_version_from_binary(work_dir: &Path) -> String {
    let exe = work_dir.join("tools").join("clients").join(mihomo_exe_name());
    if !exe.exists() {
        return String::new();
    }
    let mut cmd = Command::new(&exe);
    cmd.arg("-v")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let Ok(out) = cmd.output() else {
        return String::new();
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let text = if stdout.trim().is_empty() {
        String::from_utf8_lossy(&out.stderr).into_owned()
    } else {
        stdout.into_owned()
    };
    parse_mihomo_cli_version(&text)
}

fn parse_mihomo_cli_version(s: &str) -> String {
    for part in s.split_whitespace() {
        if !part.starts_with('v') || part.len() < 4 {
            continue;
        }
        let rest = &part[1..];
        if !rest.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            continue;
        }
        if !rest.contains('.') {
            continue;
        }
        let ver: String = part
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-')
            .collect();
        if ver.len() >= 4 {
            return ver;
        }
    }
    String::new()
}

async fn reconcile_provider(
    http: &reqwest::Client,
    provider: &str,
    order: &[usize],
    nodes: &mut [Node],
) {
    if order.is_empty() {
        return;
    }
    let ep = format!(
        "http://{CONTROLLER}/providers/proxies/{}",
        urlencoding::encode(provider)
    );
    let Ok(res) = http.get(&ep).send().await else {
        return;
    };
    let Ok(body) = res.text().await else {
        return;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) else {
        return;
    };
    let Some(arr) = v.get("proxies").and_then(|x| x.as_array()) else {
        return;
    };
    let n = arr.len().min(order.len());
    for i in 0..n {
        let idx = order[i];
        if let Some(name) = arr[i].get("name").and_then(|x| x.as_str()) {
            nodes[idx].proxy_str = name.to_string();
        }
        if let Some(ty) = arr[i].get("type").and_then(|x| x.as_str()) {
            nodes[idx].proxy_type = ty.to_string();
        }
        // 不改 group：未填分组在 start_test 统一为 Node Speedtest，禁止回退成协议名
    }
}

pub async fn launch_for_nodes(
    work_dir: &Path,
    http: &reqwest::Client,
    mut nodes: Vec<Node>,
) -> Result<LaunchResult, String> {
    if nodes.is_empty() {
        return Err("无节点".into());
    }
    let socks_port = DEFAULT_SOCKS_PORT;
    let build: ProvidersBuild = build_providers_config(&nodes, socks_port, CONTROLLER);
    if build.yaml_order.is_empty() && build.link_order.is_empty() {
        return Err("没有可交给 mihomo 的节点".into());
    }

    kill_all();
    tokio::time::sleep(Duration::from_millis(200)).await;

    if !build.yaml_provider_path.is_empty() {
        std::fs::write(
            work_dir.join(&build.yaml_provider_path),
            &build.yaml_provider_body,
        )
        .map_err(|e| format!("写 sub_yaml.yaml 失败: {e}"))?;
    }
    if !build.link_provider_path.is_empty() {
        std::fs::write(
            work_dir.join(&build.link_provider_path),
            &build.link_provider_body,
        )
        .map_err(|e| format!("写 sub_link.txt 失败: {e}"))?;
    }
    std::fs::write(work_dir.join("config.yaml"), &build.config_yaml)
        .map_err(|e| format!("写 config.yaml 失败: {e}"))?;

    spawn_mihomo(work_dir)?;
    if !wait_ready(http, 8000).await {
        return Err("mihomo 在 8s 内未就绪".into());
    }

    reconcile_provider(http, "yaml_sub", &build.yaml_order, &mut nodes).await;
    reconcile_provider(http, "link_sub", &build.link_order, &mut nodes).await;

    let version = read_version_from_binary(work_dir);
    Ok(LaunchResult {
        nodes,
        version,
        socks_port,
    })
}
