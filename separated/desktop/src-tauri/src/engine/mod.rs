//! 进程内异步测速引擎（Tokio）。
//! 协议解析与出站由 mihomo 承担；延迟/带宽测量为本模块自研实现，不依赖旧 C++ 测速管线。

mod cancel;
mod emoji;
mod export;
mod measure;
mod mihomo;
pub mod rlog;
mod session;
mod stun;
mod sub;
mod tls_check;
mod types;
mod update;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;

pub use cancel::CancelFlag;
pub use mihomo::read_version_from_binary;
pub use types::*;

use session::SessionCtrl;
use update::UpdateCaches;

/// 引擎对外句柄：Tauri 侧 `manage` 后由 api_* 命令调用。
pub struct Engine {
    work_dir: PathBuf,
    state: Arc<RwLock<EngineState>>,
    session: Arc<parking_lot::Mutex<SessionCtrl>>,
    updates: Arc<UpdateCaches>,
    /// 本地 mihomo / SOCKS / 订阅直连：绕过系统代理。
    http: reqwest::Client,
    /// GitHub 更新检查：遵循 HTTP_PROXY（启动时已注入系统代理）。
    github_http: reqwest::Client,
}

impl Engine {
    pub fn new(work_dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(work_dir.join("logs"));
        let _ = std::fs::create_dir_all(work_dir.join("results"));
        let http = reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(60))
            .build()
            .expect("engine http client");
        // 更新检查必须走系统代理：国内直连 GitHub 常失败。
        let github_http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .user_agent("NodeSpeedtest-desktop")
            .build()
            .expect("github http client");
        Self {
            work_dir,
            state: Arc::new(RwLock::new(EngineState::default())),
            session: Arc::new(parking_lot::Mutex::new(SessionCtrl::default())),
            updates: Arc::new(UpdateCaches::default()),
            http,
            github_http,
        }
    }

    pub async fn handle_get(&self, path: &str) -> Result<String, String> {
        let path = path.split('?').next().unwrap_or(path);
        match path {
            "/status" => Ok(self.status_text()),
            "/getversion" => Ok(self.version_json()),
            "/getresults" => Ok(self.results_json()),
            "/checkupdate" => {
                // 本地版本统一以 `mihomo -v` 为准（与是否已拉起内核无关）
                let local = mihomo::read_version_from_binary(&self.work_dir);
                if !local.is_empty() {
                    self.state.write().mihomo_version = local.clone();
                }
                Ok(self
                    .updates
                    .serve_mihomo(&self.github_http, local)
                    .await)
            }
            "/checkappupdate" => {
                Ok(self.updates.serve_app(&self.github_http).await)
            }
            _ => Err(format!("未知路径: {path}")),
        }
    }

    pub async fn handle_post_json(&self, path: &str, body: &str) -> Result<String, String> {
        let path = path.split('?').next().unwrap_or(path);
        match path {
            "/readsubscriptions" => self.read_subscription(body).await,
            "/start" => self.start_test(body).await,
            "/stop" => Ok(self.stop_test()),
            _ => Err(format!("未知路径: {path}")),
        }
    }

    pub async fn handle_post_file(&self, path: &str, bytes: Vec<u8>) -> Result<String, String> {
        let path = path.split('?').next().unwrap_or(path);
        match path {
            "/readfileconfig" => self.read_file_config(bytes).await,
            _ => Err(format!("未知路径: {path}")),
        }
    }

    fn status_text(&self) -> String {
        if self.state.read().running {
            "running".into()
        } else {
            "stopped".into()
        }
    }

    fn version_json(&self) -> String {
        serde_json::json!({
            "main": format!("v{}", env!("CARGO_PKG_VERSION")),
            "webapi": "0.7.0"
        })
        .to_string()
    }

    fn results_json(&self) -> String {
        let st = self.state.read();
        types::serialize_results(&st)
    }

    async fn read_subscription(&self, body: &str) -> Result<String, String> {
        if self.state.read().running {
            rlog::warn(&self.work_dir, "导入订阅被拒绝：测速进行中");
            return Ok("running".into());
        }
        let v: serde_json::Value =
            serde_json::from_str(body).map_err(|e| format!("JSON 无效: {e}"))?;
        let url = v
            .get("url")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if url.is_empty() {
            return Err("订阅链接为空".into());
        }
        let preview: String = url.chars().take(80).collect();
        rlog::info(
            &self.work_dir,
            format!("导入订阅/节点文本 len={} preview={preview}", url.len()),
        );
        // 仅「单行 http(s) 订阅 URL」走下载；trojan:// / vless:// / 多行正文 / YAML 直接解析。
        let single_line = !url.contains('\n') && !url.contains('\r');
        let is_http = url.starts_with("http://") || url.starts_with("https://");
        let content = if single_line && is_http {
            rlog::info(&self.work_dir, "拉取远程订阅…");
            match sub::fetch_subscription(&self.http, &url).await {
                Ok(c) => c,
                Err(e) => {
                    rlog::error(&self.work_dir, format!("拉取订阅失败: {e}"));
                    return Err(e);
                }
            }
        } else {
            url
        };
        self.import_content(&content).await
    }

    async fn read_file_config(&self, bytes: Vec<u8>) -> Result<String, String> {
        if self.state.read().running {
            rlog::warn(&self.work_dir, "导入文件被拒绝：测速进行中");
            return Ok("running".into());
        }
        rlog::info(
            &self.work_dir,
            format!("导入本地配置文件 bytes={}", bytes.len()),
        );
        let content = String::from_utf8_lossy(&bytes).to_string();
        match self.import_content(&content).await {
            Ok(s) => Ok(s),
            Err(e) => {
                rlog::error(&self.work_dir, format!("导入文件失败: {e}"));
                Ok("error".into())
            }
        }
    }

    async fn import_content(&self, content: &str) -> Result<String, String> {
        let mut nodes = match sub::load_subscription(content, "") {
            Ok(n) => n,
            Err(e) => {
                rlog::error(&self.work_dir, format!("解析订阅失败: {e}"));
                return Err(e);
            }
        };
        if nodes.is_empty() {
            rlog::error(&self.work_dir, "未解析到可测速节点");
            return Err("未解析到可测速节点".into());
        }
        for (i, n) in nodes.iter_mut().enumerate() {
            n.id = i as i32;
        }
        rlog::info(
            &self.work_dir,
            format!("解析完成，节点数={}", nodes.len()),
        );
        {
            let mut st = self.state.write();
            st.all_nodes = nodes;
            st.target_nodes.clear();
            st.current_id = -1;
            st.completed_ids.clear();
        }
        let work = self.work_dir.clone();
        let http = self.http.clone();
        let nodes_snapshot = self.state.read().all_nodes.clone();
        rlog::info(&self.work_dir, "启动 mihomo 内核…");
        let launched = match mihomo::launch_for_nodes(&work, &http, nodes_snapshot).await {
            Ok(l) => l,
            Err(e) => {
                rlog::error(&self.work_dir, format!("mihomo 启动失败: {e}"));
                return Err(e);
            }
        };
        {
            let mut st = self.state.write();
            st.all_nodes = launched.nodes;
            st.mihomo_version = launched.version.clone();
            st.socks_port = launched.socks_port;
        }
        rlog::info(
            &self.work_dir,
            format!(
                "mihomo 就绪 version={} socks_port={} nodes={}",
                launched.version,
                launched.socks_port,
                self.state.read().all_nodes.len()
            ),
        );
        Ok(types::serialize_web_configs(&self.state.read().all_nodes))
    }

    async fn start_test(&self, body: &str) -> Result<String, String> {
        let req: StartRequest =
            serde_json::from_str(body).map_err(|e| format!("JSON 无效: {e}"))?;
        let ping_only = req.test_mode == "TCP_PING";
        let sort = normalize_sort_method(&req.sort_method);
        let group_in = req.group.trim().to_string();

        let cancel = CancelFlag::new();
        let targets_len;
        // session + state 同临界区：检查 running / 冷却 / 匹配节点 / 置位一体，杜绝并发双开
        {
            let mut sess = self.session.lock();
            let mut st = self.state.write();
            // busy ≠ 成功启动的 "running"，避免前端误清空结果
            if st.running || sess.batch.is_some() {
                return Ok("busy".into());
            }
            if let Some(t) = st.done_at {
                if t.elapsed() < Duration::from_secs(5) {
                    return Ok("done".into());
                }
            }
            let mut targets = match_targets(&st.all_nodes, &req.configs);
            if targets.is_empty() {
                rlog::error(&self.work_dir, "开始测速失败：没有匹配到可测节点");
                return Err("没有匹配到可测节点，请重新导入订阅".into());
            }
            targets_len = targets.len();

            if let Some(old) = sess.cancel.take() {
                old.cancel();
            }
            sess.cancel = Some(cancel.clone());

            st.running = true;
            st.ping_only = ping_only;
            st.sort_method = sort.clone();
            // 填了分组 → 全部统一；未填 → 一律 Node Speedtest（不回退成 Vless 等协议名）
            let group_label = if group_in.is_empty() {
                types::DEFAULT_GROUP_LABEL.to_string()
            } else {
                group_in.clone()
            };
            st.custom_group = group_label.clone();
            for n in &mut targets {
                n.group = group_label.clone();
            }
            st.target_nodes = targets;
            st.current_id = -1;
            st.completed_ids.clear();
            st.done_at = None;
        }

        rlog::info(
            &self.work_dir,
            format!(
                "开始测速 nodes={} mode={} sort={} group={}",
                targets_len,
                if ping_only { "TCP_PING" } else { "FULL" },
                sort,
                if group_in.is_empty() {
                    "(auto)"
                } else {
                    group_in.as_str()
                }
            ),
        );

        let state = Arc::clone(&self.state);
        let work = self.work_dir.clone();
        let http = self.http.clone();
        let session = Arc::clone(&self.session);

        let batch_cancel = cancel.clone();
        let handle = tauri::async_runtime::spawn(async move {
            let outcome = session::run_batch(
                state.clone(),
                work.clone(),
                http,
                cancel,
            )
            .await;
            if let Err(e) = &outcome {
                rlog::error(&work, format!("测速批次异常: {e}"));
            } else {
                let (done, total) = {
                    let st = state.read();
                    (st.completed_ids.len(), st.target_nodes.len())
                };
                rlog::info(&work, format!("测速批次结束 completed={done}/{total}"));
            }
            // 锁序与 start_test 一致：session → state，避免与 /start 交叉死锁
            {
                let mut sess = session.lock();
                let mut st = state.write();
                st.running = false;
                st.current_id = -1;
                st.done_at = Some(Instant::now());
                if sess
                    .cancel
                    .as_ref()
                    .map(|c| c.same_as(&batch_cancel))
                    .unwrap_or(false)
                {
                    sess.cancel = None;
                }
                if sess.cancel.is_none() {
                    sess.batch = None;
                }
            }
        });
        self.session.lock().batch = Some(handle);

        Ok("running".into())
    }

    fn stop_test(&self) -> String {
        let running = self.state.read().running;
        if !running {
            return "stopped".into();
        }
        rlog::warn(&self.work_dir, "用户停止测速");
        if let Some(c) = self.session.lock().cancel.as_ref() {
            c.cancel();
        }
        {
            let mut st = self.state.write();
            st.done_at = None;
        }
        "stopping".into()
    }

    pub fn is_running(&self) -> bool {
        // running 为主；超时未死的批次靠 batch 句柄兜底（收尾时会清掉句柄）
        self.state.read().running || self.session.lock().batch.is_some()
    }

    /// 取消批次并等待其退出。超时则把 JoinHandle 放回，绝不假报 idle / 丢弃任务。
    /// 返回 true 表示已空闲。
    pub async fn await_idle(&self, timeout: Duration) -> bool {
        let handle = {
            let mut sess = self.session.lock();
            if let Some(c) = sess.cancel.as_ref() {
                c.cancel();
            }
            sess.batch.take()
        };
        let Some(mut h) = handle else {
            return !self.state.read().running;
        };
        let finished = tokio::select! {
            r = &mut h => {
                let _ = r;
                true
            }
            _ = tokio::time::sleep(timeout) => false,
        };
        if finished {
            true
        } else {
            self.session.lock().batch = Some(h);
            false
        }
    }

    /// 设置页「重启后端」：取消测速、杀 mihomo、清空会话节点。
    pub async fn restart(&self) -> Result<(), String> {
        rlog::info(&self.work_dir, "重启后端引擎…");
        if !self.await_idle(Duration::from_secs(45)).await {
            return Err("测速任务未能在时限内结束，请先停止测速后再重启".into());
        }
        mihomo::kill_all();
        {
            let mut st = self.state.write();
            st.running = false;
            st.current_id = -1;
            st.target_nodes.clear();
            st.completed_ids.clear();
            st.done_at = None;
        }
        // 若仍有节点，重新拉起内核以便继续测
        let nodes = self.state.read().all_nodes.clone();
        if !nodes.is_empty() {
            let launched =
                mihomo::launch_for_nodes(&self.work_dir, &self.http, nodes).await?;
            let mut st = self.state.write();
            st.all_nodes = launched.nodes;
            st.mihomo_version = launched.version.clone();
            st.socks_port = launched.socks_port;
            rlog::info(
                &self.work_dir,
                format!(
                    "后端已重启 mihomo={} socks={}",
                    launched.version, launched.socks_port
                ),
            );
        } else {
            rlog::info(&self.work_dir, "后端已重启（无节点，未拉起 mihomo）");
        }
        Ok(())
    }

    pub fn shutdown(&self) {
        if let Some(c) = self.session.lock().cancel.as_ref() {
            c.cancel();
        }
        mihomo::kill_all();
    }

    /// 清数据等 release 路径使用；debug 下 clear_app_data 直接拒绝，故允许未引用。
    #[cfg_attr(debug_assertions, allow(dead_code))]
    pub async fn shutdown_and_wait(&self, timeout: Duration) -> Result<(), String> {
        if !self.await_idle(timeout).await {
            return Err("测速任务未能在时限内结束，请稍后重试".into());
        }
        mihomo::kill_all();
        Ok(())
    }
}

fn normalize_sort_method(s: &str) -> String {
    let lower = s.to_lowercase();
    lower.replace("reverse_", "r")
}

fn match_targets(all: &[Node], configs: &[NodeConfigIn]) -> Vec<Node> {
    let mut out = Vec::new();
    for cfg in configs {
        let c = &cfg.config;
        if let Some(n) = all.iter().find(|x| {
            x.group == c.group
                && x.remarks == c.remarks
                && x.server == c.server
                && x.port == c.server_port
        }) {
            out.push(n.clone());
        }
    }
    for (i, n) in out.iter_mut().enumerate() {
        n.id = i as i32;
    }
    out
}
