use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

#[derive(Default)]
struct CacheInner {
    latest: String,
    error: String,
    has_update: bool,
    at: Option<Instant>,
}

pub struct UpdateCaches {
    mihomo: Arc<Mutex<CacheInner>>,
    app: Arc<Mutex<CacheInner>>,
    mihomo_refreshing: Arc<AtomicBool>,
    app_refreshing: Arc<AtomicBool>,
}

impl Default for UpdateCaches {
    fn default() -> Self {
        Self {
            mihomo: Arc::new(Mutex::new(CacheInner::default())),
            app: Arc::new(Mutex::new(CacheInner::default())),
            mihomo_refreshing: Arc::new(AtomicBool::new(false)),
            app_refreshing: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl UpdateCaches {
    pub async fn serve_mihomo(&self, http: &reqwest::Client, local: String) -> String {
        self.serve(
            http,
            Arc::clone(&self.mihomo),
            Arc::clone(&self.mihomo_refreshing),
            "https://api.github.com/repos/MetaCubeX/mihomo/releases/latest",
            "https://github.com/MetaCubeX/mihomo/releases/latest",
            local,
        )
        .await
    }

    pub async fn serve_app(&self, http: &reqwest::Client) -> String {
        let local = format!("v{}", env!("CARGO_PKG_VERSION"));
        self.serve(
            http,
            Arc::clone(&self.app),
            Arc::clone(&self.app_refreshing),
            "https://api.github.com/repos/ECYCloud/node-speedtest/releases/latest",
            "https://github.com/ECYCloud/node-speedtest/releases/latest",
            local,
        )
        .await
    }

    async fn serve(
        &self,
        http: &reqwest::Client,
        cache: Arc<Mutex<CacheInner>>,
        refreshing: Arc<AtomicBool>,
        api_url: &'static str,
        release_url: &'static str,
        local: String,
    ) -> String {
        const TTL: Duration = Duration::from_secs(1800);
        let (latest, error, has_update, stale) = {
            let c = cache.lock();
            let stale = c.at.map(|t| t.elapsed() > TTL).unwrap_or(true);
            (c.latest.clone(), c.error.clone(), c.has_update, stale)
        };

        if stale && !refreshing.swap(true, Ordering::SeqCst) {
            let http = http.clone();
            let cache = Arc::clone(&cache);
            let refreshing = Arc::clone(&refreshing);
            let local_for = local.clone();
            tauri::async_runtime::spawn(async move {
                let (new_latest, new_error, new_has) =
                    fetch_latest(&http, api_url, &local_for).await;
                {
                    let mut c = cache.lock();
                    c.latest = new_latest;
                    c.error = new_error;
                    c.has_update = new_has;
                    c.at = Some(Instant::now());
                }
                refreshing.store(false, Ordering::SeqCst);
            });
        }

        let mut error = error;
        let at_empty = cache.lock().at.is_none();
        if latest.is_empty() && error.is_empty() && at_empty {
            error = "正在检查...".into();
        }

        serde_json::json!({
            "local": local,
            "latest": latest,
            "has_update": has_update,
            "release_url": release_url,
            "error": error,
        })
        .to_string()
    }
}

async fn fetch_latest(
    http: &reqwest::Client,
    api_url: &str,
    local: &str,
) -> (String, String, bool) {
    let res = match http
        .get(api_url)
        .header("User-Agent", "NodeSpeedtest-desktop")
        .send()
        .await
    {
        Ok(r) => r,
        Err(_) => {
            return (
                String::new(),
                "GitHub API 无响应(网络受限或被防火墙拦截)".into(),
                false,
            )
        }
    };
    let body = match res.text().await {
        Ok(b) => b,
        Err(_) => return (String::new(), "读取 GitHub 响应失败".into(), false),
    };
    let v: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return (String::new(), "GitHub API 返回了非预期的内容".into(), false),
    };
    let latest = v
        .get("tag_name")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let has = !latest.is_empty() && !local.is_empty() && compare_version(&latest, local) > 0;
    (latest, String::new(), has)
}

fn compare_version(a: &str, b: &str) -> i32 {
    let parse = |s: &str| -> Vec<u32> {
        s.trim_start_matches('v')
            .split(|c: char| !c.is_ascii_digit())
            .filter(|x| !x.is_empty())
            .filter_map(|x| x.parse().ok())
            .collect()
    };
    let va = parse(a);
    let vb = parse(b);
    let n = va.len().max(vb.len());
    for i in 0..n {
        let x = *va.get(i).unwrap_or(&0);
        let y = *vb.get(i).unwrap_or(&0);
        if x > y {
            return 1;
        }
        if x < y {
            return -1;
        }
    }
    0
}
