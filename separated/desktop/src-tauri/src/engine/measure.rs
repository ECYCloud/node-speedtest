//! 现代化测速：mihomo 出站 + 经 SOCKS5 的并发带宽估计。
//!
//! - 延迟：SOCKS5 unified（预热+计时）多次采样，≥3 有效时剔除最大值再平均；失败再走 /delay
//! - 带宽：丢弃预热阶段、EMA 平滑；测满固定窗，不因稳定提前结束（吞吐 20 格对齐）
//! - 多连接并发下载，吞吐按「有效测量窗」内字节/时间计算

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use parking_lot::{Mutex, RwLock};
use tokio::task::JoinSet;

use super::cancel::CancelFlag;
use super::mihomo;
use super::stun;
use super::tls_check::{self, TlsVerifyState};
use super::types::{speed_calc, EngineState, GeoBlock, Node, DEFAULT_TEST_FILE, LATENCY_URL};

/// 延迟探测次数（与 C++ LATENCY_PROBE_COUNT 一致；多于 1 次才能在 UI 看到累进过程）。
const LATENCY_PROBES: usize = 5;
/// 并发下载连接数。
const STREAMS: usize = 6;
/// 预热时长：慢启动 / 握手尖峰不计入均值；预热期内不写入 raw_speed。
const WARMUP: Duration = Duration::from_secs(1);
/// raw_speed 一格对应时长（与导出吞吐柱 20 格一致）。
const SAMPLE_SLOT_MS: u64 = 500;
/// 固定总时长 = 预热 + 20 格采样，保证每节点吞吐柱同长、同刻度。
const MEASURE_MAX: Duration =
    Duration::from_millis(1000 + SAMPLE_SLOT_MS * 20);
/// 采样间隔（写入 raw_speed 供前端火花图）。
const TICK: Duration = Duration::from_millis(250);
/// EMA 平滑系数。
const EMA_ALPHA: f64 = 0.35;

/// 把 shared 快照立刻写进 EngineState，供 /getresults 轮询读取（不经 200ms sync 延迟）。
fn publish_node(state: &RwLock<EngineState>, node: &Node) {
    let mut st = state.write();
    if let Some(slot) = st.target_nodes.iter_mut().find(|x| x.id == node.id) {
        *slot = node.clone();
    }
}

pub async fn probe_latency(
    http: &reqwest::Client,
    socks_port: u16,
    proxy_name: &str,
    cancel: &CancelFlag,
    shared: &Arc<Mutex<Node>>,
    state: &RwLock<EngineState>,
) -> (f64, [i32; 10], [i32; 6]) {
    let mut samples: Vec<i32> = Vec::with_capacity(LATENCY_PROBES);
    let mut rawms = [0i32; 10];
    let mut sum_ms: i64 = 0;

    for i in 0..LATENCY_PROBES {
        if cancel.is_cancelled() {
            break;
        }
        // 优先经已切换的 SOCKS5 做 unified 口径（预热 HEAD + 计时 HEAD）。
        // 直接打 mihomo /delay 时，内核 unified-delay 的第二次 HEAD 常失败，
        // 会退回「含完整握手」的墙钟时间（常见 1–3s），且后续探测易 503/504，
        // 表现为 UI「1次 + 两千 ms」。
        let ms = match socks5_unified_delay(socks_port).await {
            Some(v) => Some(v),
            None => mihomo::measure_delay(http, proxy_name).await,
        };
        let Some(ms) = ms else {
            continue;
        };
        rawms[i] = ms;
        samples.push(ms);
        sum_ms += ms as i64;
        let running = sum_ms as f64 / samples.len() as f64;
        let mut raw_ping = [0i32; 6];
        for j in 0..6.min(LATENCY_PROBES) {
            raw_ping[j] = rawms[j];
        }
        let snap = {
            let mut n = shared.lock();
            n.avg_ping_ms = running;
            n.site_ping_ms = running;
            n.raw_site_ping = rawms;
            n.raw_ping = raw_ping;
            n.clone()
        };
        publish_node(state, &snap);
    }

    let mut latency = -1.0f64;
    if !samples.is_empty() {
        // 与 C++ main.cpp 对齐：≥3 个有效样本时剔除最大值再平均。
        if samples.len() >= 3 {
            let worst = *samples.iter().max().unwrap_or(&0) as i64;
            let full: i64 = samples.iter().map(|&x| x as i64).sum();
            latency = (full - worst) as f64 / (samples.len() - 1) as f64;
        } else {
            latency = sum_ms as f64 / samples.len() as f64;
        }
    }

    let mut raw_ping = [0i32; 6];
    for i in 0..6.min(LATENCY_PROBES) {
        raw_ping[i] = rawms[i];
    }
    (latency, rawms, raw_ping)
}

/// 经 SOCKS5 复现 mihomo `unified-delay`：先预热一次，再测第二次 RTT。
async fn socks5_unified_delay(socks_port: u16) -> Option<i32> {
    let proxy = format!("socks5h://127.0.0.1:{socks_port}");
    let client = reqwest::Client::builder()
        .no_proxy()
        .proxy(reqwest::Proxy::all(&proxy).ok()?)
        .timeout(Duration::from_secs(8))
        .pool_max_idle_per_host(4)
        .http1_only()
        .build()
        .ok()?;

    let warm = client.head(LATENCY_URL).send().await.ok()?;
    if !(warm.status().is_success() || warm.status().as_u16() == 204) {
        return None;
    }
    let _ = warm.bytes().await;

    let t0 = Instant::now();
    let res = client.head(LATENCY_URL).send().await.ok()?;
    if !(res.status().is_success() || res.status().as_u16() == 204) {
        return None;
    }
    let _ = res.bytes().await;
    let ms = t0.elapsed().as_millis() as i32;
    if ms <= 0 {
        None
    } else {
        Some(ms)
    }
}

async fn fetch_geoip_cancellable(
    http: &reqwest::Client,
    socks_port: u16,
    server: &str,
    cancel: &CancelFlag,
) -> (GeoBlock, GeoBlock) {
    if cancel.is_cancelled() {
        return (GeoBlock::default(), GeoBlock::default());
    }
    let cancel_wait = async {
        while !cancel.is_cancelled() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    tokio::select! {
        _ = cancel_wait => (GeoBlock::default(), GeoBlock::default()),
        r = fetch_geoip(http, socks_port, server) => r,
    }
}

pub async fn fetch_geoip(
    http: &reqwest::Client,
    socks_port: u16,
    server: &str,
) -> (GeoBlock, GeoBlock) {
    let inbound_fut = async {
        let url = format!("https://api.ip.sb/geoip/{server}");
        let send = tokio::time::timeout(Duration::from_secs(4), http.get(&url).send()).await;
        let res = match send {
            Ok(Ok(r)) => Some(r),
            _ => None,
        };
        parse_geo_response(res, server).await
    };
    let outbound_fut = async {
        let proxy = format!("socks5h://127.0.0.1:{socks_port}");
        let Ok(px) = reqwest::Proxy::all(&proxy) else {
            return GeoBlock::default();
        };
        let client = match reqwest::Client::builder()
            .no_proxy()
            .proxy(px)
            .timeout(Duration::from_secs(4))
            .build()
        {
            Ok(c) => c,
            Err(_) => return GeoBlock::default(),
        };
        parse_geo_response(client.get("https://api.ip.sb/geoip").send().await.ok(), "").await
    };
    tokio::join!(inbound_fut, outbound_fut)
}

async fn parse_geo_response(res: Option<reqwest::Response>, fallback_addr: &str) -> GeoBlock {
    let na = || GeoBlock {
        address: fallback_addr.into(),
        info: "N/A N/A, N/A".into(),
        ..Default::default()
    };
    let Some(res) = res else {
        return na();
    };
    let Ok(v) = res.json::<serde_json::Value>().await else {
        return na();
    };
    let ip = v
        .get("ip")
        .and_then(|x| x.as_str())
        .unwrap_or(fallback_addr)
        .to_string();
    let country = v
        .get("country")
        .and_then(|x| x.as_str())
        .unwrap_or("N/A")
        .to_string();
    let city = v
        .get("city")
        .and_then(|x| x.as_str())
        .unwrap_or("N/A")
        .to_string();
    let org = v
        .get("organization")
        .or_else(|| v.get("isp"))
        .and_then(|x| x.as_str())
        .unwrap_or("N/A")
        .to_string();
    let cc = v
        .get("country_code")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    GeoBlock {
        address: ip.clone(),
        info: format!("{country} {city}, {org}"),
        ip,
        country,
        city,
        organization: org,
        country_code: cc,
    }
}

/// UDP/NAT：经 SOCKS5 UDP ASSOCIATE + Classic STUN（RFC 3489）。
/// Blocked/Unknown 再探一次，抑制偶发丢包误判；整体封顶避免拖死单节点。
pub async fn detect_nat(socks_port: u16, cancel: &CancelFlag) -> String {
    if cancel.is_cancelled() {
        return "Unknown".into();
    }
    let run = async {
        let first = stun::detect_nat_thru_socks5(socks_port).await;
        if first != "Blocked" && first != "Unknown" {
            return first;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
        let second = stun::detect_nat_thru_socks5(socks_port).await;
        if second == "Blocked" || second == "Unknown" {
            first
        } else {
            second
        }
    };
    let cancel_wait = async {
        while !cancel.is_cancelled() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    tokio::select! {
        _ = cancel_wait => "Unknown".into(),
        r = tokio::time::timeout(Duration::from_secs(10), run) => match r {
            Ok(s) => s,
            Err(_) => "Unknown".into(),
        },
    }
}

async fn verify_tls_cancellable(socks_port: u16, cancel: &CancelFlag) -> TlsVerifyState {
    if cancel.is_cancelled() {
        return TlsVerifyState::NotApplicable;
    }
    let cancel_wait = async {
        while !cancel.is_cancelled() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    tokio::select! {
        _ = cancel_wait => TlsVerifyState::NotApplicable,
        r = tls_check::verify_tls_via_socks5(
            socks_port,
            tls_check::TLS_VERIFY_HOST,
            tls_check::TLS_VERIFY_PORT,
        ) => r,
    }
}

pub struct DownloadProgress {
    pub raw_speed: [u64; 20],
    pub avg_speed: String,
    pub max_speed: String,
    pub total_recv_bytes: u64,
}

/// 现代化带宽测量：预热丢弃 + EMA；固定测满 MEASURE_MAX，采满 20 格。
pub async fn perform_download(
    socks_port: u16,
    test_file: &str,
    cancel: &CancelFlag,
    on_tick: impl Fn(DownloadProgress),
) -> DownloadProgress {
    let url = if test_file.is_empty() {
        DEFAULT_TEST_FILE
    } else {
        test_file
    };
    let received = Arc::new(AtomicU64::new(0));
    let proxy = format!("socks5h://127.0.0.1:{socks_port}");
    let workers_done = CancelFlag::new();

    let mut set = JoinSet::new();
    for _ in 0..STREAMS {
        let received = Arc::clone(&received);
        let stop = workers_done.clone();
        let proxy = proxy.clone();
        let url = url.to_string();
        set.spawn(async move {
            download_worker(&proxy, &url, &received, &stop).await;
        });
    }

    let start = Instant::now();
    let mut last_bytes = 0u64;
    let mut last_tick = start;
    let mut ema: f64 = 0.0;
    let mut ema_ready = false;
    let mut measure_bytes_at_warmup_end = 0u64;
    let mut warmup_ended = false;
    let mut raw_speed = [0u64; 20];
    let mut sample_i = 0usize;
    // 最高速度只取 raw_speed 槽位峰值（与 UI/导出吞吐柱同源），
    // 不用 250ms tick 上的 peak_ema——后者会高于任何已展示采样，造成「测时 94、结果 103」。
    let mut peak_raw: u64 = 0;

    loop {
        tokio::time::sleep(TICK).await;
        if cancel.is_cancelled() {
            break;
        }

        let now = Instant::now();
        let cur = received.load(Ordering::Relaxed);
        let dt = now.duration_since(last_tick).as_secs_f64().max(1e-6);
        let inst = (cur.saturating_sub(last_bytes) as f64) / dt; // B/s
        last_bytes = cur;
        last_tick = now;

        if !ema_ready {
            ema = inst;
            ema_ready = true;
        } else {
            ema = EMA_ALPHA * inst + (1.0 - EMA_ALPHA) * ema;
        }

        let elapsed = now.duration_since(start);

        // 预热结束：锁定起点字节，峰值从预热后开始统计
        if !warmup_ended && elapsed >= WARMUP {
            warmup_ended = true;
            measure_bytes_at_warmup_end = cur;
        }

        // 预热结束后按固定槽位写入 raw_speed（每 SAMPLE_SLOT_MS 一格，共 20 格）
        if warmup_ended && sample_i < 20 {
            let after = elapsed.saturating_sub(WARMUP);
            let idx = (after.as_millis() as u64 / SAMPLE_SLOT_MS).min(19) as usize;
            while sample_i <= idx && sample_i < 20 {
                let v = ema.max(0.0) as u64;
                raw_speed[sample_i] = v;
                peak_raw = peak_raw.max(v);
                sample_i += 1;
            }
        }

        let measured_bytes = if warmup_ended {
            cur.saturating_sub(measure_bytes_at_warmup_end)
        } else {
            0
        };
        let measure_secs = if warmup_ended {
            elapsed.saturating_sub(WARMUP).as_secs_f64().max(1e-3)
        } else {
            1e-3
        };
        let avg_bps = if warmup_ended && measured_bytes > 0 {
            measured_bytes as f64 / measure_secs
        } else {
            0.0
        };

        on_tick(DownloadProgress {
            raw_speed,
            avg_speed: if avg_bps > 0.0 {
                speed_calc(avg_bps)
            } else {
                "N/A".into()
            },
            max_speed: if peak_raw > 0 {
                speed_calc(peak_raw as f64)
            } else {
                "N/A".into()
            },
            total_recv_bytes: cur,
        });

        if elapsed >= MEASURE_MAX {
            break;
        }
    }

    // 正常跑满时补齐未写入的尾格（取消时仍保留已采到的前缀）
    if !cancel.is_cancelled() {
        while sample_i < 20 {
            let v = ema.max(0.0) as u64;
            raw_speed[sample_i] = v;
            peak_raw = peak_raw.max(v);
            sample_i += 1;
        }
        // 预热刚结束瞬时速率常为 0，会留下首格空洞；用后续首个有效值回填。
        if let Some(&first) = raw_speed.iter().find(|&&v| v > 0) {
            for v in &mut raw_speed {
                if *v == 0 {
                    *v = first;
                } else {
                    break;
                }
            }
            peak_raw = peak_raw.max(first);
        }
    }

    workers_done.cancel();
    // 协作取消无法打断卡住的 HTTP send；短等后 abort，避免每节点多挂数十秒。
    let drained = tokio::time::timeout(Duration::from_millis(500), async {
        while set.join_next().await.is_some() {}
    })
    .await;
    if drained.is_err() {
        set.abort_all();
        while set.join_next().await.is_some() {}
    }

    let cur = received.load(Ordering::Relaxed);
    let elapsed = start.elapsed();
    let measured_bytes = if warmup_ended {
        cur.saturating_sub(measure_bytes_at_warmup_end)
    } else {
        cur
    };
    let measure_secs = if warmup_ended {
        elapsed.saturating_sub(WARMUP).as_secs_f64().max(1e-3)
    } else {
        elapsed.as_secs_f64().max(1e-3)
    };
    let avg_bps = measured_bytes as f64 / measure_secs;
    // 终值再扫一遍 raw_speed，保证与数组完全一致（含首格回填）
    peak_raw = raw_speed.iter().copied().max().unwrap_or(0).max(peak_raw);
    let mut avg = speed_calc(avg_bps);
    let mut max_s = speed_calc(peak_raw as f64);
    if measured_bytes == 0 {
        avg = "N/A".into();
        max_s = "N/A".into();
    }
    DownloadProgress {
        raw_speed,
        avg_speed: avg,
        max_speed: max_s,
        total_recv_bytes: cur,
    }
}

async fn download_worker(
    proxy: &str,
    url: &str,
    received: &AtomicU64,
    cancel: &CancelFlag,
) {
    let Ok(proxy) = reqwest::Proxy::all(proxy) else {
        return;
    };
    let Ok(client) = reqwest::Client::builder()
        .no_proxy()
        .proxy(proxy)
        // 略长于 MEASURE_MAX 即可；过长会在取消后拖死 join
        .timeout(Duration::from_secs(12))
        .pool_max_idle_per_host(4)
        .build()
    else {
        return;
    };
    loop {
        if cancel.is_cancelled() {
            break;
        }
        let res = match client.get(url).send().await {
            Ok(r) => r,
            Err(_) => {
                tokio::time::sleep(Duration::from_millis(150)).await;
                continue;
            }
        };
        let mut stream = res.bytes_stream();
        while let Some(chunk) = stream.next().await {
            if cancel.is_cancelled() {
                return;
            }
            if let Ok(b) = chunk {
                received.fetch_add(b.len() as u64, Ordering::Relaxed);
            } else {
                break;
            }
        }
    }
}

pub async fn single_test(
    work_dir: &Path,
    http: &reqwest::Client,
    socks_port: u16,
    shared: Arc<Mutex<Node>>,
    state: Arc<RwLock<EngineState>>,
    ping_only: bool,
    cancel: &CancelFlag,
) {
    let proxy_str = shared.lock().proxy_str.clone();
    let server = shared.lock().server.clone();

    if cancel.is_cancelled() {
        return;
    }
    if !mihomo::ensure_alive(work_dir, http).await {
        let mut n = shared.lock();
        fail_node(&mut n);
        publish_node(&state, &n);
        return;
    }
    if proxy_str.is_empty() {
        let snap = {
            let mut n = shared.lock();
            n.online = false;
            n.clone()
        };
        publish_node(&state, &snap);
        return;
    }
    let mut switched = mihomo::switch_proxy(http, &proxy_str).await;
    if !switched {
        tokio::time::sleep(Duration::from_millis(200)).await;
        switched = mihomo::switch_proxy(http, &proxy_str).await;
    }
    if !switched {
        let mut n = shared.lock();
        fail_node(&mut n);
        publish_node(&state, &n);
        return;
    }
    // 出站切换后短暂稳定（过长会显著拖慢整批）
    tokio::time::sleep(Duration::from_millis(200)).await;

    let (latency, raw_site, raw_ping) =
        probe_latency(http, socks_port, &proxy_str, cancel, &shared, &state).await;
    {
        let snap = {
            let mut n = shared.lock();
            if latency < 0.0 {
                n.avg_ping_ms = 0.0;
                n.site_ping_ms = 0.0;
            } else {
                // 终值用截尾均值覆盖探测过程中的累进均值
                n.avg_ping_ms = latency;
                n.site_ping_ms = latency;
            }
            n.raw_site_ping = raw_site;
            n.raw_ping = raw_ping;
            n.pk_loss = "0.00%".into();
            n.clone()
        };
        publish_node(&state, &snap);
    }

    if ping_only {
        // 仅延迟：并行补 NAT / TLS / Geo，不再串行拉满超时
        let geo_fut = fetch_geoip_cancellable(http, socks_port, &server, cancel);
        let nat_fut = detect_nat(socks_port, cancel);
        let tls_fut = verify_tls_cancellable(socks_port, cancel);
        let ((inbound, outbound), nat, tls) = tokio::join!(geo_fut, nat_fut, tls_fut);
        let snap = {
            let mut n = shared.lock();
            n.inbound_geo = inbound;
            n.outbound_geo = outbound;
            n.nat_type = nat;
            n.tls_verified = tls.as_str().into();
            n.online = latency >= 0.0;
            n.clone()
        };
        publish_node(&state, &snap);
        return;
    }
    if cancel.is_cancelled() {
        return;
    }

    // NAT 必须在带宽下载前单独做：与下载并行会抢同一条 UDP/出站，
    // 易把可达节点误判为 Blocked（同节点换协议时尤其容易抖）。
    let nat = detect_nat(socks_port, cancel).await;
    {
        let snap = {
            let mut n = shared.lock();
            n.nat_type = nat.clone();
            n.clone()
        };
        publish_node(&state, &snap);
    }
    if cancel.is_cancelled() {
        return;
    }

    // TLS 必须在带宽下载前单独做：与 6 路下载并行时易被挤成超时，
    // 误标 NotApplicable，页脚再被渲染成「未核实」。
    let tls = verify_tls_cancellable(socks_port, cancel).await;
    {
        let snap = {
            let mut n = shared.lock();
            n.tls_verified = if latency < 0.0 && tls == TlsVerifyState::NotApplicable {
                TlsVerifyState::NotApplicable.as_str().into()
            } else {
                tls.as_str().into()
            };
            n.clone()
        };
        publish_node(&state, &snap);
    }
    if cancel.is_cancelled() {
        return;
    }

    {
        shared.lock().test_file = DEFAULT_TEST_FILE.into();
    }
    let test_file = DEFAULT_TEST_FILE.to_string();
    let shared_tick = Arc::clone(&shared);
    let state_tick = Arc::clone(&state);
    let dl_cancel = cancel.child_token();
    let geo_fut = fetch_geoip_cancellable(http, socks_port, &server, cancel);
    let dl_fut = perform_download(socks_port, &test_file, &dl_cancel, move |p| {
        let snap = {
            let mut n = shared_tick.lock();
            n.raw_speed = p.raw_speed;
            n.avg_speed = p.avg_speed;
            n.max_speed = p.max_speed;
            n.total_recv_bytes = p.total_recv_bytes;
            n.clone()
        };
        publish_node(&state_tick, &snap);
    });

    let ((inbound, outbound), final_prog) = tokio::join!(geo_fut, dl_fut);

    {
        let snap = {
            let mut n = shared.lock();
            n.inbound_geo = inbound;
            n.outbound_geo = outbound;
            n.nat_type = nat;
            n.raw_speed = final_prog.raw_speed;
            n.avg_speed = final_prog.avg_speed;
            n.max_speed = final_prog.max_speed;
            n.total_recv_bytes = final_prog.total_recv_bytes;
            n.online = final_prog.total_recv_bytes > 0 || latency >= 0.0;
            n.clone()
        };
        publish_node(&state, &snap);
    }
}

fn fail_node(node: &mut Node) {
    node.avg_ping_ms = 0.0;
    node.site_ping_ms = 0.0;
    node.pk_loss = "100.00%".into();
    node.avg_speed = "N/A".into();
    node.max_speed = "N/A".into();
    node.online = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measure_window_fills_twenty_spark_slots() {
        assert_eq!(SAMPLE_SLOT_MS, 500);
        assert_eq!(WARMUP, Duration::from_secs(1));
        assert_eq!(MEASURE_MAX, Duration::from_secs(11));
        let sample_window_ms = MEASURE_MAX.saturating_sub(WARMUP).as_millis() as u64;
        assert!(
            sample_window_ms / SAMPLE_SLOT_MS >= 20,
            "有效采样窗 {sample_window_ms}ms 不足以采满 20 格"
        );
    }

    #[test]
    fn sample_index_covers_full_array_after_warmup() {
        let mut raw = [0u64; 20];
        let mut sample_i = 0usize;
        let after_ms = MEASURE_MAX.saturating_sub(WARMUP).as_millis() as u64;
        let idx = (after_ms / SAMPLE_SLOT_MS).min(19) as usize;
        while sample_i <= idx && sample_i < 20 {
            raw[sample_i] = 1;
            sample_i += 1;
        }
        while sample_i < 20 {
            raw[sample_i] = 1;
            sample_i += 1;
        }
        assert_eq!(sample_i, 20);
        assert!(raw.iter().all(|&v| v > 0));
    }

    /// 真实下载实测：mihomo DIRECT + Cloudflare，必须跑满约 10s 且 raw_speed 20 格有值。
    #[tokio::test(flavor = "multi_thread")]
    async fn live_download_fills_twenty_slots_no_early_exit() {
        let mihomo = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("engine")
            .join("tools")
            .join("clients")
            .join(if cfg!(windows) {
                "mihomo.exe"
            } else {
                "mihomo"
            });
        assert!(
            mihomo.is_file(),
            "缺少 mihomo: {}",
            mihomo.display()
        );

        let dir = std::env::temp_dir().join(format!(
            "ns-measure-live-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        let cfg = dir.join("config.yaml");
        // 专用端口，避免撞上正在运行的桌面实例 65432
        const PORT: u16 = 61987;
        std::fs::write(
            &cfg,
            format!(
                "socks-port: {PORT}\nallow-lan: false\nmode: global\nlog-level: silent\n\
                 dns:\n  enable: true\n  listen: 0.0.0.0:0\n  enhanced-mode: fake-ip\n\
                 proxy-groups:\n  - name: GLOBAL\n    type: select\n    proxies: [DIRECT]\n"
            ),
        )
        .expect("write config");

        let mut child = std::process::Command::new(&mihomo)
            .arg("-d")
            .arg(&dir)
            .arg("-f")
            .arg(&cfg)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn mihomo");

        // 等 SOCKS 就绪
        let mut ready = false;
        for _ in 0..40 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if tokio::net::TcpStream::connect(("127.0.0.1", PORT))
                .await
                .is_ok()
            {
                ready = true;
                break;
            }
        }
        if !ready {
            let _ = child.kill();
            panic!("mihomo SOCKS :{PORT} 未就绪");
        }

        let cancel = CancelFlag::new();
        let t0 = Instant::now();
        let prog = perform_download(PORT, DEFAULT_TEST_FILE, &cancel, |_| {}).await;
        let elapsed = t0.elapsed();
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&dir);

        let filled = prog.raw_speed.iter().filter(|&&v| v > 0).count();
        eprintln!(
            "live download: elapsed={:.2}s avg={} max={} traffic={} filled={}/20 raw={:?}",
            elapsed.as_secs_f64(),
            prog.avg_speed,
            prog.max_speed,
            prog.total_recv_bytes,
            filled,
            prog.raw_speed
        );

        assert!(
            elapsed >= Duration::from_secs(10),
            "未测满固定窗，疑似提前结束: {:.2}s",
            elapsed.as_secs_f64()
        );
        assert!(
            elapsed <= Duration::from_secs(15),
            "测速过久异常: {:.2}s",
            elapsed.as_secs_f64()
        );
        assert_eq!(
            filled, 20,
            "raw_speed 未采满 20 格: filled={filled} raw={:?}",
            prog.raw_speed
        );
        assert!(
            prog.total_recv_bytes > 0,
            "下载流量为 0，网络或 DIRECT 出站异常"
        );
    }
}
