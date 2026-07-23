//! 结果图：现代紧凑表格（Node Speedtest）。
//! 布局：品牌头 + 数据表 + 底部摘要（节点/消耗流量/TLS/地区运营商/测试时间与时长）。
//! 分辨率倍率读 pref.ini 的 image_scale（默认 4，高清原图）。

use std::cmp::Ordering;
use std::fs::File;
use std::io::BufWriter;
use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::imageops::{self, FilterType as ResizeFilter};
use image::{ImageEncoder, Rgb, RgbImage};

use super::emoji;
use super::types::{speed_calc, stream_to_bps, Node};

/// 导出页脚用的本机信息（测试机地区/运营商/开始时间/时长）。
#[derive(Debug, Clone, Default)]
pub struct ExportMeta {
    pub region_line: String,
    pub isp: String,
    /// 测试开始时间（与 C++ g_test_start_time 对齐，非导出瞬间）。
    pub test_time: String,
    /// 整批测速墙钟时长，格式 HH:MM:SS。
    pub duration: String,
}

const VERSION: &str = env!("CARGO_PKG_VERSION");
/// 国旗宽高比 3:2（规范旗帜比例，非 emoji 正方形）。
const FLAG_ASPECT_NUM: i32 = 3;
const FLAG_ASPECT_DEN: i32 = 2;

const C_BG: Rgb<u8> = Rgb([248, 250, 252]);
const C_SURFACE: Rgb<u8> = Rgb([255, 255, 255]);
const C_FG: Rgb<u8> = Rgb([15, 23, 42]);
const C_MUTE: Rgb<u8> = Rgb([100, 116, 139]);
const C_BORDER: Rgb<u8> = Rgb([226, 232, 240]);
const C_HEADER: Rgb<u8> = Rgb([241, 245, 249]);
const C_ZEBRA: Rgb<u8> = Rgb([248, 250, 252]);
const C_PRIMARY: Rgb<u8> = Rgb([37, 99, 235]);
const C_SUCCESS: Rgb<u8> = Rgb([22, 163, 74]);
const C_WARNING: Rgb<u8> = Rgb([217, 119, 6]);
const C_DANGER: Rgb<u8> = Rgb([220, 38, 38]);
const C_ACCENT_BAR: Rgb<u8> = Rgb([37, 99, 235]);

/// 网速色停靠点（B/s → RGB）：慢=浅红，快=深红，中间线性插值。
const SPEED_COLOR_STOPS: &[(f64, [u8; 3])] = &[
    (0.0, [252, 165, 165]),                  // 起点：浅红
    (512.0 * 1024.0, [248, 113, 113]),       // ~512KB
    (2.0 * 1024.0 * 1024.0, [239, 68, 68]),  // ~2MB
    (8.0 * 1024.0 * 1024.0, [220, 38, 38]),  // ~8MB
    (20.0 * 1024.0 * 1024.0, [185, 28, 28]), // ~20MB
    (40.0 * 1024.0 * 1024.0, [127, 29, 29]), // ≥40MB：深红
];

/// 吞吐柱满高对应的最低绝对刻度（B/s）。
/// 若仅按本批 peak 归一化，单节点十几 B/s 时柱体会撑满整行，看起来像高速。
const SPARK_MIN_FULL_BPS: f64 = 1024.0 * 1024.0;

pub fn save_results(
    work_dir: &Path,
    nodes: &[Node],
    group: &str,
    sort_method: &str,
    mihomo_version: &str,
    meta: &ExportMeta,
) -> Result<(), String> {
    let mut sorted = nodes.to_vec();
    sort_nodes(&mut sorted, sort_method);
    let results_dir = work_dir.join("results");
    std::fs::create_dir_all(&results_dir).map_err(|e| e.to_string())?;
    let stamp = now_stamp();
    let base = format!("{}-{}", sanitize_filename(group), stamp);
    write_log(&results_dir.join(format!("{base}.log")), &sorted, group)?;
    write_png(
        &results_dir.join(format!("{base}.png")),
        work_dir,
        &sorted,
        group,
        mihomo_version,
        meta,
    )?;
    Ok(())
}

/// 拉取本机出口地区/运营商；时间与时长由调用方在批次开始/结束时传入。
pub async fn collect_export_meta(test_start_time: String, elapsed: Duration) -> ExportMeta {
    let (region_line, isp) = fetch_local_region_isp().await;
    ExportMeta {
        region_line,
        isp,
        test_time: if test_start_time.is_empty() {
            format_local_test_time()
        } else {
            test_start_time
        },
        duration: format_duration_hms(elapsed),
    }
}

/// 本地墙钟时间字符串（含时区），供批次开始时采样。
pub fn format_local_test_time() -> String {
    format_local_test_time_impl()
}

fn format_duration_hms(d: Duration) -> String {
    let secs = d.as_secs();
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

async fn fetch_local_region_isp() -> (String, String) {
    let Ok(client) = reqwest::Client::builder()
        .no_proxy()
        .local_address(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
        .timeout(Duration::from_secs(6))
        .user_agent("Mozilla/5.0 NodeSpeedtest")
        .build()
    else {
        return (String::new(), String::new());
    };
    for attempt in 1..=2u32 {
        match fetch_local_geo_once(&client).await {
            Ok((region, isp)) => return (region, isp),
            Err(e) => {
                eprintln!("[export] local geo attempt {attempt}/2 failed: {e}");
                if attempt < 2 {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }
    (String::new(), String::new())
}

async fn fetch_local_geo_once(client: &reqwest::Client) -> Result<(String, String), String> {
    let url = "http://ip-api.com/json/?lang=zh-CN&fields=country,regionName,city,isp,status,message";
    let res = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("请求失败: {e}"))?;
    let json: serde_json::Value = res.json().await.map_err(|e| format!("解析失败: {e}"))?;
    if json.get("status").and_then(|v| v.as_str()) != Some("success") {
        let msg = json
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("未知错误");
        return Err(msg.into());
    }
    let pick = |k: &str| -> String {
        json.get(k)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    let mut parts = Vec::new();
    for key in ["country", "regionName", "city"] {
        let v = pick(key);
        if !v.is_empty() && !parts.iter().any(|x: &String| x == &v) {
            parts.push(v);
        }
    }
    Ok((parts.join(" "), translate_isp_short(&pick("isp"))))
}

fn translate_isp_short(raw: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }
    let lower = raw.to_lowercase();
    let rules: &[(&str, &str)] = &[
        ("chinanet", "中国电信"),
        ("china telecom", "中国电信"),
        ("chinaunicom", "中国联通"),
        ("china unicom", "中国联通"),
        ("china169", "中国联通"),
        ("unicom", "中国联通"),
        ("chinamobile", "中国移动"),
        ("china mobile", "中国移动"),
        ("cmcc", "中国移动"),
        ("cloudflare", "Cloudflare"),
    ];
    for (kw, zh) in rules {
        if lower.contains(kw) {
            return (*zh).into();
        }
    }
    raw.to_string()
}

fn format_local_test_time_impl() -> String {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::SYSTEMTIME;
        use windows_sys::Win32::System::SystemInformation::GetLocalTime;
        use windows_sys::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
        unsafe {
            let mut st = SYSTEMTIME {
                wYear: 0,
                wMonth: 0,
                wDayOfWeek: 0,
                wDay: 0,
                wHour: 0,
                wMinute: 0,
                wSecond: 0,
                wMilliseconds: 0,
            };
            GetLocalTime(&mut st);
            let mut tz = TIME_ZONE_INFORMATION {
                Bias: 0,
                StandardName: [0; 32],
                StandardDate: SYSTEMTIME {
                    wYear: 0,
                    wMonth: 0,
                    wDayOfWeek: 0,
                    wDay: 0,
                    wHour: 0,
                    wMinute: 0,
                    wSecond: 0,
                    wMilliseconds: 0,
                },
                StandardBias: 0,
                DaylightName: [0; 32],
                DaylightDate: SYSTEMTIME {
                    wYear: 0,
                    wMonth: 0,
                    wDayOfWeek: 0,
                    wDay: 0,
                    wHour: 0,
                    wMinute: 0,
                    wSecond: 0,
                    wMilliseconds: 0,
                },
                DaylightBias: 0,
            };
            // 1 = STANDARD, 2 = DAYLIGHT（windows-sys 未导出后两者常量）
            let id = GetTimeZoneInformation(&mut tz);
            let mut bias = tz.Bias;
            if id == 2 {
                bias += tz.DaylightBias;
            } else {
                bias += tz.StandardBias;
            }
            // Bias：UTC 以西的分钟数；中国为 -480 → UTC+8
            let total_min = -bias;
            let sign = if total_min >= 0 { '+' } else { '-' };
            let abs = total_min.unsigned_abs();
            let tz_label = if abs % 60 == 0 {
                format!("UTC{sign}{}", abs / 60)
            } else {
                format!("UTC{sign}{}:{:02}", abs / 60, abs % 60)
            };
            // 中国标准时区额外标 CST，与常见测速图一致
            let tz_show = if total_min == 480 {
                "CST".into()
            } else {
                tz_label
            };
            return format!(
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02} ({tz_show})",
                st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond
            );
        }
    }
    #[cfg(not(windows))]
    {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let days = secs / 86400;
        let tod = secs % 86400;
        let (y, m, d) = civil_from_days(days as i64);
        format!(
            "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} (UTC)",
            tod / 3600,
            (tod % 3600) / 60,
            tod % 60
        )
    }
}

#[cfg(not(windows))]
fn civil_from_days(mut z: i64) -> (i32, u32, u32) {
    z += 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

fn now_stamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let tod = secs % 86400;
    format!(
        "{:05}-{:02}{:02}{:02}",
        secs / 86400,
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}

fn sanitize_filename(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect();
    if t.is_empty() {
        "Results".into()
    } else {
        t
    }
}

fn sort_nodes(nodes: &mut [Node], method: &str) {
    let cmp_f = |a: f64, b: f64| a.partial_cmp(&b).unwrap_or(Ordering::Equal);
    match method {
        "speed" => nodes.sort_by(|a, b| cmp_f(stream_to_bps(&a.avg_speed), stream_to_bps(&b.avg_speed))),
        "rspeed" => {
            nodes.sort_by(|a, b| cmp_f(stream_to_bps(&b.avg_speed), stream_to_bps(&a.avg_speed)))
        }
        "maxspeed" => {
            nodes.sort_by(|a, b| cmp_f(stream_to_bps(&a.max_speed), stream_to_bps(&b.max_speed)))
        }
        "rmaxspeed" => {
            nodes.sort_by(|a, b| cmp_f(stream_to_bps(&b.max_speed), stream_to_bps(&a.max_speed)))
        }
        "ping" => nodes.sort_by(|a, b| cmp_f(a.avg_ping_ms, b.avg_ping_ms)),
        "rping" => nodes.sort_by(|a, b| cmp_f(b.avg_ping_ms, a.avg_ping_ms)),
        _ => {}
    }
}

fn write_log(path: &Path, nodes: &[Node], group: &str) -> Result<(), String> {
    let mut out = String::new();
    out.push_str("[Basic]\n");
    out.push_str(&format!("Tester={group}\n\n"));
    for n in nodes {
        out.push_str(&format!("[{}^{}]\n", n.group, n.remarks));
        out.push_str(&format!("AvgPing={:.2}\n", n.avg_ping_ms));
        out.push_str(&format!("SitePing={:.2}\n", n.site_ping_ms));
        out.push_str(&format!("AvgSpeed={}\n", n.avg_speed));
        out.push_str(&format!("MaxSpeed={}\n", n.max_speed));
        out.push_str(&format!("UsedTraffic={}\n", n.total_recv_bytes));
        out.push_str(&format!("NatType={}\n", n.nat_type));
        out.push_str(&format!("TlsVerified={}\n", n.tls_verified));
        out.push_str(&format!(
            "RawSpeed={}\n\n",
            n.raw_speed
                .iter()
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    std::fs::write(path, out).map_err(|e| format!("写 log 失败: {e}"))
}

fn read_image_scale(work_dir: &Path) -> i32 {
    let path = work_dir.join("pref.ini");
    let Ok(text) = std::fs::read_to_string(path) else {
        return 4;
    };
    for line in text.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("image_scale=") {
            let n: i32 = v.trim().parse().unwrap_or(4);
            return n.clamp(1, 6);
        }
    }
    4
}

fn write_png(
    path: &Path,
    work_dir: &Path,
    nodes: &[Node],
    group: &str,
    mihomo_version: &str,
    meta: &ExportMeta,
) -> Result<(), String> {
    let font_data = std::fs::read(
        work_dir
            .join("tools")
            .join("misc")
            .join("SourceHanSansCN-Medium.otf"),
    )
    .map_err(|e| format!("读字体失败: {e}"))?;
    let font = FontRef::try_from_slice(&font_data).map_err(|_| "解析字体失败".to_string())?;

    let s = read_image_scale(work_dir);
    // 紧凑版：行高贴近字号，避免大片上下空白
    let pad = 12 * s;
    let fs_brand = 9.0 * s as f32;
    let fs_title = 15.0 * s as f32;
    let fs = 10.0 * s as f32;
    let fs_foot = 9.0 * s as f32;
    let brand_sc = PxScale::from(fs_brand);
    let title_sc = PxScale::from(fs_title);
    let sc = PxScale::from(fs);
    let foot_sc = PxScale::from(fs_foot);

    // 行高/国旗一律按汉字真实墨迹，不用整 em（思源黑体字怀上下空白很大）
    let ink_h = glyph_ink_height(&font, sc, '国').max(8);
    let header_block = 34 * s;
    let foot_ink_h = glyph_ink_height(&font, foot_sc, '国').max(8);
    let foot_line = foot_ink_h + 3 * s;
    let cell_pad = 6 * s;
    let idx_w = 30 * s;
    let spark_min = 96 * s;
    let flag_h = ink_h;
    let flag_w = (flag_h * FLAG_ASPECT_NUM / FLAG_ASPECT_DEN).max(1);
    let name_gap = 4 * s;
    let name_lpad = 6 * s;

    let n_count = nodes.len();
    let banner = if group.trim().is_empty() {
        super::types::resolve_test_group("", nodes)
    } else {
        group.to_string()
    };

    let mut onlines = 0i32;
    let mut traffic = 0u64;
    let mut show_speed = false;
    let mut show_udp = false;
    let mut tls_ok = 0i32;
    let mut tls_fail = 0i32;
    let mut peak = 0u64;
    let mut name_parts = Vec::with_capacity(n_count);

    for node in nodes {
        if node.online {
            onlines += 1;
        }
        traffic += node.total_recv_bytes;
        if !node.avg_speed.is_empty() && node.avg_speed != "N/A" {
            show_speed = true;
        }
        if should_show_udp_col(node) {
            show_udp = true;
        }
        match node.tls_verified.as_str() {
            "Verified" => tls_ok += 1,
            "Failed" => tls_fail += 1,
            _ => {}
        }
        for &v in &node.raw_speed {
            peak = peak.max(v);
        }
        name_parts.push(emoji::resolve_flag(
            &node.remarks,
            &node.outbound_geo.country_code,
            &node.inbound_geo.country_code,
        ));
    }

    // 有吞吐列时加高行，柱体才能铺满可读
    let row_h = if show_speed {
        ink_h + 10 * s
    } else {
        ink_h + 5 * s
    };
    let head_h = row_h;

    let measure = |scale: PxScale, text: &str| text_w(&font, scale, text).ceil() as i32;
    let col_w = |header: &str, cells: &[String], min_w: i32| {
        let mut w = measure(sc, header);
        for c in cells {
            w = w.max(measure(sc, c));
        }
        (w + cell_pad).max(min_w)
    };

    let name_cells: Vec<String> = name_parts.iter().map(|(_, r)| r.clone()).collect();
    let type_cells: Vec<String> = nodes
        .iter()
        .map(|n| {
            if n.proxy_type.is_empty() {
                "-".into()
            } else {
                n.proxy_type.clone()
            }
        })
        .collect();
    let ping_cells: Vec<String> = nodes.iter().map(|n| format_ping(n.site_ping_ms)).collect();
    let avg_cells: Vec<String> = nodes.iter().map(|n| n.avg_speed.clone()).collect();
    let max_cells: Vec<String> = nodes.iter().map(|n| n.max_speed.clone()).collect();
    let udp_cells: Vec<String> = nodes
        .iter()
        .map(|n| udp_label(&n.nat_type).to_string())
        .collect();

    let mut name_w = col_w("节点", &name_cells, 80 * s) + flag_w + name_gap + name_lpad;
    let type_w = col_w("类型", &type_cells, 56 * s);
    let ping_w = col_w("延迟", &ping_cells, 72 * s);
    let (avg_w, max_w, spark_w) = if show_speed {
        (
            col_w("平均", &avg_cells, 72 * s),
            col_w("最高", &max_cells, 72 * s),
            spark_min.max(measure(sc, "吞吐") + cell_pad),
        )
    } else {
        (0, 0, 0)
    };
    let udp_w = if show_udp {
        col_w("UDP", &udp_cells, 72 * s)
    } else {
        0
    };

    let mut content_w = idx_w + name_w + type_w + ping_w + avg_w + max_w + spark_w + udp_w;
    let summary = format!(
        "{n_count} 节点 · {onlines}/{n_count} 在线 · 消耗流量 {}",
        speed_calc(traffic as f64)
    );
    let tls_foot = tls_footer_state(n_count as i32, tls_ok, tls_fail);
    let mut location = String::new();
    if !meta.region_line.is_empty() {
        location.push_str(&meta.region_line);
    }
    if !meta.isp.is_empty() {
        if !location.is_empty() {
            location.push_str(" · ");
        }
        location.push_str(&meta.isp);
    }
    let time_base = if meta.test_time.is_empty() {
        format_local_test_time()
    } else {
        meta.test_time.clone()
    };
    let dur = if meta.duration.is_empty() {
        "00:00:00".into()
    } else {
        meta.duration.clone()
    };
    let time_line = format!("测试时间: {time_base} · 测试时长: {dur}");
    let kernel = if mihomo_version.is_empty() {
        "mihomo".into()
    } else if mihomo_version.starts_with('v') || mihomo_version.starts_with('V') {
        format!("mihomo {mihomo_version}")
    } else {
        format!("mihomo v{mihomo_version}")
    };
    let meta_line = format!("Node Speedtest {VERSION} · 内核 {kernel}");

    // 底部行数：摘要 + (TLS) + (地区) + 时间 + 软件/内核
    let mut foot_lines = 3i32; // summary, time, meta
    if tls_foot.is_some() {
        foot_lines += 1;
    }
    if !location.is_empty() {
        foot_lines += 1;
    }
    let foot_block = 10 * s + foot_lines * foot_line;

    let tls_text_w = tls_foot
        .map(|(_, msg)| measure(foot_sc, msg) + 22 * s)
        .unwrap_or(0);
    let need_w = (measure(title_sc, &banner) + 24 * s)
        .max(measure(foot_sc, &summary) + 24 * s)
        .max(tls_text_w + 24 * s)
        .max(measure(foot_sc, &location) + 24 * s)
        .max(measure(foot_sc, &time_line) + 24 * s)
        .max(measure(foot_sc, &meta_line) + 24 * s);
    if need_w > content_w {
        name_w += need_w - content_w;
        content_w = need_w;
    }

    let total_w = content_w + pad * 2;
    let table_h = head_h + row_h * n_count as i32;
    let total_h = pad + header_block + 6 * s + table_h + foot_block + pad;

    let mut img = RgbImage::from_pixel(total_w as u32, total_h as u32, C_BG);

    fill_rect(&mut img, 0, 0, 3 * s, total_h, C_ACCENT_BAR);

    let mut y = pad;
    draw_text(
        &mut img,
        &font,
        pad,
        y + (fs_brand as i32),
        brand_sc,
        "NODE SPEEDTEST",
        C_PRIMARY,
    );
    let ver = format!("v{VERSION}");
    let vw = measure(brand_sc, &ver);
    draw_text(
        &mut img,
        &font,
        total_w - pad - vw,
        y + (fs_brand as i32),
        brand_sc,
        &ver,
        C_MUTE,
    );
    y += 14 * s;
    draw_text(
        &mut img,
        &font,
        pad,
        y + (fs_title as i32) * 4 / 5,
        title_sc,
        &banner,
        C_FG,
    );
    y = pad + header_block + 6 * s;

    let table_x = pad;
    let table_y = y;
    fill_round_rect(&mut img, table_x, table_y, content_w, table_h, 6 * s, C_SURFACE);
    stroke_round_rect(&mut img, table_x, table_y, content_w, table_h, 6 * s, C_BORDER, 1);

    let col_x = [
        table_x,
        table_x + idx_w,
        table_x + idx_w + name_w,
        table_x + idx_w + name_w + type_w,
        table_x + idx_w + name_w + type_w + ping_w,
        table_x + idx_w + name_w + type_w + ping_w + avg_w,
        table_x + idx_w + name_w + type_w + ping_w + avg_w + max_w,
        table_x + idx_w + name_w + type_w + ping_w + avg_w + max_w + spark_w,
        table_x + content_w,
    ];

    // 表头
    fill_round_rect_top(&mut img, table_x, table_y, content_w, head_h, 6 * s, C_HEADER);
    cell_text(&mut img, &font, sc, col_x[0], col_x[1], table_y, head_h, fs as i32, "#", C_MUTE, true, s);
    cell_text(&mut img, &font, sc, col_x[1], col_x[2], table_y, head_h, fs as i32, "节点", C_MUTE, true, s);
    cell_text(&mut img, &font, sc, col_x[2], col_x[3], table_y, head_h, fs as i32, "类型", C_MUTE, true, s);
    cell_text(&mut img, &font, sc, col_x[3], col_x[4], table_y, head_h, fs as i32, "延迟", C_MUTE, true, s);
    if show_speed {
        cell_text(&mut img, &font, sc, col_x[4], col_x[5], table_y, head_h, fs as i32, "平均", C_MUTE, true, s);
        cell_text(&mut img, &font, sc, col_x[5], col_x[6], table_y, head_h, fs as i32, "最高", C_MUTE, true, s);
        cell_text(&mut img, &font, sc, col_x[6], col_x[7], table_y, head_h, fs as i32, "吞吐", C_MUTE, true, s);
    }
    if show_udp {
        cell_text(&mut img, &font, sc, col_x[7], col_x[8], table_y, head_h, fs as i32, "UDP", C_MUTE, true, s);
    }

    let mut ry = table_y + head_h;
    for i in 0..n_count {
        let node = &nodes[i];
        if i % 2 == 1 {
            fill_rect(&mut img, table_x + 1, ry, content_w - 2, row_h, C_ZEBRA);
        }
        cell_text(
            &mut img,
            &font,
            sc,
            col_x[0],
            col_x[1],
            ry,
            row_h,
            fs as i32,
            &format!("{}", i + 1),
            C_MUTE,
            true,
            s,
        );

        {
            let (flag, rest) = &name_parts[i];
            let mut tx = col_x[1] + name_lpad;
            // 按汉字墨迹居中；国旗高度=墨迹高，中线对齐
            let sample = rest.chars().next().unwrap_or('国');
            let baseline = ink_baseline_in_row(&font, sc, ry, row_h, sample);
            let mid = ink_visual_mid(&font, sc, baseline, sample);
            if let Some(f) = flag {
                if blit_flag_aligned(
                    &mut img,
                    work_dir,
                    f,
                    tx,
                    mid,
                    flag_w as u32,
                    flag_h as u32,
                ) {
                    tx += flag_w + name_gap;
                }
            }
            let name = truncate(rest, 32);
            draw_text(&mut img, &font, tx, baseline, sc, &name, C_FG);
        }

        cell_text(
            &mut img, &font, sc, col_x[2], col_x[3], ry, row_h, fs as i32, &type_cells[i], C_FG, true, s,
        );
        cell_text(
            &mut img,
            &font,
            sc,
            col_x[3],
            col_x[4],
            ry,
            row_h,
            fs as i32,
            &ping_cells[i],
            ping_color(node.site_ping_ms),
            true,
            s,
        );

        if show_speed {
            cell_text(
                &mut img,
                &font,
                sc,
                col_x[4],
                col_x[5],
                ry,
                row_h,
                fs as i32,
                &avg_cells[i],
                speed_text_color(&node.avg_speed),
                true,
                s,
            );
            cell_text(
                &mut img,
                &font,
                sc,
                col_x[5],
                col_x[6],
                ry,
                row_h,
                fs as i32,
                &max_cells[i],
                speed_text_color(&node.max_speed),
                true,
                s,
            );
            // 吞吐柱：固定 20 格（与测速采样槽对齐）；高度按 max(本批峰值, 1MB/s)
            let sx0 = col_x[6] + 5 * s;
            let sw = (col_x[7] - col_x[6]) - 10 * s;
            let pad_y = s.max(1);
            let sh = (row_h - 2 * pad_y).max(ink_h);
            let base = ry + row_h - pad_y;
            if peak == 0 {
                cell_text(&mut img, &font, sc, col_x[6], col_x[7], ry, row_h, fs as i32, "—", C_MUTE, true, s);
            } else {
                let gap = (s / 2).max(1);
                let per = (sw / 20).max(2);
                let bar_w = (per - gap).max(1);
                let denom = (peak as f64).max(SPARK_MIN_FULL_BPS);
                fill_rect(
                    &mut img,
                    sx0,
                    base,
                    (per * 20 - gap).min(sw).max(1),
                    1.max(s / 5),
                    C_BORDER,
                );
                for j in 0..20 {
                    let v = node.raw_speed[j];
                    if v == 0 {
                        continue;
                    }
                    // 开方缓压，避免仅尖峰贴顶、其余柱过矮
                    let ratio = (v as f64 / denom).clamp(0.0, 1.0).sqrt();
                    let hh = (ratio * sh as f64).round() as i32;
                    // 极低速只留 1px 痕迹，禁止再抬到 2px 以上造成"虚高"
                    let hh = if hh < 1 { 1 } else { hh };
                    fill_rect(
                        &mut img,
                        sx0 + j as i32 * per,
                        base - hh,
                        bar_w,
                        hh,
                        speed_color_for_bps(v as f64),
                    );
                }
            }
        }

        if show_udp {
            cell_text(
                &mut img,
                &font,
                sc,
                col_x[7],
                col_x[8],
                ry,
                row_h,
                fs as i32,
                &udp_cells[i],
                udp_color(&node.nat_type),
                true,
                s,
            );
        }

        hline(&mut img, table_x + 1, table_x + content_w - 2, ry + row_h, C_BORDER);
        ry += row_h;
    }

    // 底部摘要（原顶部摘要条下移）
    let mut fy = table_y + table_h + 8 * s;
    let foot_base = |y: i32| ink_baseline_in_row(&font, foot_sc, y, foot_line, '测');
    draw_text(
        &mut img,
        &font,
        pad,
        foot_base(fy),
        foot_sc,
        &summary,
        C_MUTE,
    );
    fy += foot_line;

    if let Some((tls_kind, tls_msg)) = tls_foot {
        // TLS 图标裁透明边后，高度/中线对齐页脚汉字墨迹
        let icon_sz = foot_ink_h;
        let tls_base = foot_base(fy);
        let tls_mid = ink_visual_mid(&font, foot_sc, tls_base, '已');
        let badge_x = blit_tls_icon_aligned(
            &mut img,
            work_dir,
            tls_kind,
            pad,
            tls_mid,
            icon_sz,
        );
        let tls_color = match tls_kind {
            TlsFootKind::AllOk => C_SUCCESS,
            TlsFootKind::Partial => C_WARNING,
            TlsFootKind::AllBad => C_DANGER,
        };
        draw_text(
            &mut img,
            &font,
            badge_x,
            tls_base,
            foot_sc,
            tls_msg,
            tls_color,
        );
        fy += foot_line;
    }

    if !location.is_empty() {
        draw_text(
            &mut img,
            &font,
            pad,
            foot_base(fy),
            foot_sc,
            &format!("测试机: {location}"),
            C_MUTE,
        );
        fy += foot_line;
    }

    draw_text(
        &mut img,
        &font,
        pad,
        foot_base(fy),
        foot_sc,
        &time_line,
        C_MUTE,
    );
    fy += foot_line;
    draw_text(
        &mut img,
        &font,
        pad,
        foot_base(fy),
        foot_sc,
        &meta_line,
        C_MUTE,
    );

    save_png(path, &img)
}

#[derive(Clone, Copy)]
enum TlsFootKind {
    AllOk,
    Partial,
    AllBad,
}

/// 与 C++ renderer 对齐：全员 NotApplicable（未得出结论）时不画 TLS 行，
/// 禁止把「核对超时/未完成」渲染成红叉「未核实」。
fn tls_footer_state(
    _n_count: i32,
    tls_ok: i32,
    tls_fail: i32,
) -> Option<(TlsFootKind, &'static str)> {
    if tls_ok == 0 && tls_fail == 0 {
        return None;
    }
    if tls_fail == 0 && tls_ok > 0 {
        Some((
            TlsFootKind::AllOk,
            "已核实全部节点的 TLS 证书",
        ))
    } else if tls_ok > 0 {
        Some((
            TlsFootKind::Partial,
            "已核实部分节点的 TLS 证书",
        ))
    } else {
        Some((
            TlsFootKind::AllBad,
            "未核实全部节点的 TLS 证书",
        ))
    }
}

/// 本地 Material Icons：裁透明边后缩放到 `size`，中线对齐 `mid_y`。
fn blit_tls_icon_aligned(
    img: &mut RgbImage,
    work_dir: &Path,
    kind: TlsFootKind,
    x: i32,
    mid_y: i32,
    size: i32,
) -> i32 {
    let name = match kind {
        TlsFootKind::AllOk => "check_circle",
        TlsFootKind::Partial => "warning",
        TlsFootKind::AllBad => "cancel",
    };
    let size = size.max(8) as u32;
    let path = work_dir
        .join("tools")
        .join("misc")
        .join("icons")
        .join(format!("{name}.png"));
    let Ok(src) = image::open(&path) else {
        return x + size as i32 + 6;
    };
    let trimmed = trim_rgba_alpha(&src.to_rgba8(), 8);
    let resized = imageops::resize(&trimmed, size, size, ResizeFilter::Lanczos3);
    let top = mid_y - size as i32 / 2;
    blit_rgba(img, &resized, x, top);
    x + size as i32 + 6
}

fn format_ping(ms: f64) -> String {
    if ms <= 0.0 {
        "—".into()
    } else {
        format!("{:.0} ms", ms)
    }
}

fn ping_color(ms: f64) -> Rgb<u8> {
    if ms <= 0.0 {
        C_MUTE
    } else if ms < 150.0 {
        C_SUCCESS
    } else if ms < 400.0 {
        C_WARNING
    } else {
        C_DANGER
    }
}

fn speed_text_color(speed: &str) -> Rgb<u8> {
    let bps = stream_to_bps(speed);
    if bps <= 0.0 {
        C_MUTE
    } else {
        speed_color_for_bps(bps)
    }
}

/// 按硬编码网速档位插值：越快越深红。
fn speed_color_for_bps(bps: f64) -> Rgb<u8> {
    let bps = bps.max(0.0);
    let last = SPEED_COLOR_STOPS.len() - 1;
    if bps <= SPEED_COLOR_STOPS[0].0 {
        return Rgb(SPEED_COLOR_STOPS[0].1);
    }
    if bps >= SPEED_COLOR_STOPS[last].0 {
        return Rgb(SPEED_COLOR_STOPS[last].1);
    }
    for i in 0..last {
        let (a_bps, a_rgb) = SPEED_COLOR_STOPS[i];
        let (b_bps, b_rgb) = SPEED_COLOR_STOPS[i + 1];
        if bps >= a_bps && bps <= b_bps {
            let t = if b_bps > a_bps {
                ((bps - a_bps) / (b_bps - a_bps)) as f32
            } else {
                1.0
            };
            return Rgb([
                lerp_u8(a_rgb[0], b_rgb[0], t),
                lerp_u8(a_rgb[1], b_rgb[1], t),
                lerp_u8(a_rgb[2], b_rgb[2], t),
            ]);
        }
    }
    Rgb(SPEED_COLOR_STOPS[last].1)
}

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    let t = t.clamp(0.0, 1.0);
    (a as f32 + (b as f32 - a as f32) * t).round() as u8
}

/// Unknown/Blocked 也要出列，避免结果图整列消失像「没测 UDP」。
fn should_show_udp_col(node: &crate::engine::types::Node) -> bool {
    !node.nat_type.is_empty()
}

fn udp_label(nat: &str) -> &'static str {
    match nat {
        "FullCone" => "完全支持",
        "RestrictedCone" => "受限支持",
        "PortRestrictedCone" => "端口受限",
        "Symmetric" => "对称受限",
        "Blocked" => "不支持",
        _ => "未知",
    }
}

fn udp_color(nat: &str) -> Rgb<u8> {
    match nat {
        "FullCone" => C_SUCCESS,
        "RestrictedCone" => C_PRIMARY,
        "PortRestrictedCone" => C_WARNING,
        "Symmetric" | "Blocked" => C_DANGER,
        _ => C_MUTE,
    }
}

fn truncate(s: &str, max: usize) -> String {
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i >= max {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

/// 采样字相对基线的墨迹顶/底（outline px_bounds）。
fn glyph_ink_rel(font: &FontRef<'_>, scale: PxScale, ch: char) -> Option<(f32, f32)> {
    let scaled = font.as_scaled(scale);
    let mut glyph = scaled.scaled_glyph(ch);
    glyph.position = ab_glyph::point(0.0, 0.0);
    let ol = font.outline_glyph(glyph)?;
    let b = ol.px_bounds();
    Some((b.min.y, b.max.y))
}

fn glyph_ink_height(font: &FontRef<'_>, scale: PxScale, ch: char) -> i32 {
    glyph_ink_rel(font, scale, ch)
        .map(|(t, b)| (b - t).ceil() as i32)
        .unwrap_or_else(|| scale.y.ceil() as i32)
}

/// 行内基线：把采样字墨迹垂直居中到行高。
fn ink_baseline_in_row(
    font: &FontRef<'_>,
    scale: PxScale,
    top_y: i32,
    row_h: i32,
    sample: char,
) -> i32 {
    let row_mid = top_y as f32 + row_h as f32 * 0.5;
    if let Some((t, b)) = glyph_ink_rel(font, scale, sample) {
        let ink_mid = (t + b) * 0.5; // 相对基线=0
        return (row_mid - ink_mid).round() as i32;
    }
    let sf = font.as_scaled(scale);
    (row_mid + (sf.ascent() + sf.descent()) * 0.5).round() as i32
}

fn ink_visual_mid(font: &FontRef<'_>, scale: PxScale, baseline: i32, sample: char) -> i32 {
    if let Some((t, b)) = glyph_ink_rel(font, scale, sample) {
        return (baseline as f32 + (t + b) * 0.5).round() as i32;
    }
    let sf = font.as_scaled(scale);
    let top = baseline as f32 - sf.ascent();
    let bot = baseline as f32 - sf.descent();
    ((top + bot) * 0.5).round() as i32
}

fn cell_text(
    img: &mut RgbImage,
    font: &FontRef<'_>,
    scale: PxScale,
    x_left: i32,
    x_right: i32,
    top_y: i32,
    row_h: i32,
    _font_size: i32,
    txt: &str,
    color: Rgb<u8>,
    centered: bool,
    s: i32,
) {
    let tw = text_w(font, scale, txt).ceil() as i32;
    let x = if centered {
        x_left + ((x_right - x_left) - tw) / 2
    } else {
        x_left + 8 * s
    };
    let sample = txt.chars().find(|c| !c.is_ascii_whitespace()).unwrap_or('国');
    draw_text(
        img,
        font,
        x,
        ink_baseline_in_row(font, scale, top_y, row_h, sample),
        scale,
        txt,
        color,
    );
}

/// 裁掉 RGBA 四周近透明边，避免国旗/图标“画布”大于可见内容。
fn trim_rgba_alpha(src: &image::RgbaImage, alpha_min: u8) -> image::RgbaImage {
    let (w, h) = src.dimensions();
    let mut min_x = w;
    let mut min_y = h;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut any = false;
    for y in 0..h {
        for x in 0..w {
            if src.get_pixel(x, y)[3] >= alpha_min {
                any = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    if !any {
        return src.clone();
    }
    imageops::crop_imm(src, min_x, min_y, max_x - min_x + 1, max_y - min_y + 1).to_image()
}

fn blit_rgba(img: &mut RgbImage, src: &image::RgbaImage, x: i32, y: i32) {
    for py in 0..src.height() {
        for px in 0..src.width() {
            let p = src.get_pixel(px, py);
            let a = p[3] as f32 / 255.0;
            if a < 0.05 {
                continue;
            }
            let dx = x + px as i32;
            let dy = y + py as i32;
            if dx < 0 || dy < 0 {
                continue;
            }
            let (dx, dy) = (dx as u32, dy as u32);
            if dx >= img.width() || dy >= img.height() {
                continue;
            }
            let dst = *img.get_pixel(dx, dy);
            img.put_pixel(
                dx,
                dy,
                Rgb([
                    blend_u8(dst[0], p[0], a),
                    blend_u8(dst[1], p[1], a),
                    blend_u8(dst[2], p[2], a),
                ]),
            );
        }
    }
}

/// 国旗：裁透明边后铺满 3:2 目标矩形，垂直中线对齐 `mid_y`。
fn blit_flag_aligned(
    img: &mut RgbImage,
    work_dir: &Path,
    flag: &str,
    x: i32,
    mid_y: i32,
    w: u32,
    h: u32,
) -> bool {
    let w = w.max(1);
    let h = h.max(1);
    let render_px = w.max(h).saturating_mul(2).max(48);
    let Some(rgba) = emoji::render_flag(work_dir, flag, render_px) else {
        return false;
    };
    let trimmed = trim_rgba_alpha(&rgba, 12);
    let resized = imageops::resize(&trimmed, w, h, ResizeFilter::Lanczos3);
    let top = mid_y - h as i32 / 2;
    blit_rgba(img, &resized, x, top);
    true
}

fn save_png(path: &Path, img: &RgbImage) -> Result<(), String> {
    let f = File::create(path).map_err(|e| format!("创建 PNG 失败: {e}"))?;
    // Best：尽量保真，避免额外有损压缩观感发糊
    PngEncoder::new_with_quality(BufWriter::new(f), CompressionType::Best, FilterType::Adaptive)
        .write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|e| format!("写 PNG 失败: {e}"))
}

fn text_w(font: &FontRef<'_>, scale: PxScale, text: &str) -> f32 {
    let scaled = font.as_scaled(scale);
    text.chars()
        .map(|ch| scaled.h_advance(scaled.scaled_glyph(ch).id))
        .sum()
}

fn draw_text(
    img: &mut RgbImage,
    font: &FontRef<'_>,
    x: i32,
    y: i32,
    scale: PxScale,
    text: &str,
    color: Rgb<u8>,
) {
    let scaled = font.as_scaled(scale);
    let mut caret = x as f32;
    for ch in text.chars() {
        let u = ch as u32;
        if (0x1F1E6..=0x1F1FF).contains(&u) {
            caret += scale.y * 0.5;
            continue;
        }
        let mut glyph = scaled.scaled_glyph(ch);
        glyph.position = ab_glyph::point(caret, y as f32);
        let adv = scaled.h_advance(glyph.id);
        if let Some(ol) = font.outline_glyph(glyph) {
            let b = ol.px_bounds();
            ol.draw(|gx, gy, v| {
                if v < 0.04 {
                    return;
                }
                let px = b.min.x as i32 + gx as i32;
                let py = b.min.y as i32 + gy as i32;
                if px < 0 || py < 0 {
                    return;
                }
                let (px, py) = (px as u32, py as u32);
                if px >= img.width() || py >= img.height() {
                    return;
                }
                let src = *img.get_pixel(px, py);
                img.put_pixel(
                    px,
                    py,
                    Rgb([
                        blend_u8(src[0], color[0], v),
                        blend_u8(src[1], color[1], v),
                        blend_u8(src[2], color[2], v),
                    ]),
                );
            });
        }
        caret += adv;
    }
}

fn fill_rect(img: &mut RgbImage, x: i32, y: i32, w: i32, h: i32, color: Rgb<u8>) {
    if w <= 0 || h <= 0 {
        return;
    }
    for py in y..y + h {
        for px in x..x + w {
            put(img, px, py, color);
        }
    }
}

fn fill_round_rect(img: &mut RgbImage, x: i32, y: i32, w: i32, h: i32, r: i32, color: Rgb<u8>) {
    if w <= 0 || h <= 0 {
        return;
    }
    let r = r.min(w / 2).min(h / 2).max(0);
    for py in y..y + h {
        for px in x..x + w {
            if inside_round_rect(px, py, x, y, w, h, r) {
                put(img, px, py, color);
            }
        }
    }
}

fn fill_round_rect_top(img: &mut RgbImage, x: i32, y: i32, w: i32, h: i32, r: i32, color: Rgb<u8>) {
    if w <= 0 || h <= 0 {
        return;
    }
    let r = r.min(w / 2).min(h).max(0);
    for py in y..y + h {
        for px in x..x + w {
            let lx = px - x;
            let ly = py - y;
            let in_top_corner = (lx < r && ly < r && (r - lx) * (r - lx) + (r - ly) * (r - ly) > r * r)
                || (lx >= w - r
                    && ly < r
                    && (lx - (w - r - 1)) * (lx - (w - r - 1)) + (r - ly) * (r - ly) > r * r);
            if !in_top_corner {
                put(img, px, py, color);
            }
        }
    }
}

fn stroke_round_rect(
    img: &mut RgbImage,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    r: i32,
    color: Rgb<u8>,
    thick: i32,
) {
    if w <= 0 || h <= 0 {
        return;
    }
    let r = r.min(w / 2).min(h / 2).max(0);
    for t in 0..thick.max(1) {
        for py in (y - t)..(y + h + t) {
            for px in (x - t)..(x + w + t) {
                let in_outer = inside_round_rect(px, py, x - t, y - t, w + 2 * t, h + 2 * t, r + t);
                let in_inner = inside_round_rect(
                    px,
                    py,
                    x + t + 1,
                    y + t + 1,
                    w - 2 * t - 2,
                    h - 2 * t - 2,
                    (r - t - 1).max(0),
                );
                if in_outer && !in_inner {
                    put(img, px, py, color);
                }
            }
        }
    }
}

fn inside_round_rect(px: i32, py: i32, x: i32, y: i32, w: i32, h: i32, r: i32) -> bool {
    if px < x || py < y || px >= x + w || py >= y + h {
        return false;
    }
    if r <= 0 {
        return true;
    }
    let lx = px - x;
    let ly = py - y;
    let corners = [
        (lx < r && ly < r, r - lx, r - ly),
        (lx >= w - r && ly < r, lx - (w - r - 1), r - ly),
        (lx < r && ly >= h - r, r - lx, ly - (h - r - 1)),
        (lx >= w - r && ly >= h - r, lx - (w - r - 1), ly - (h - r - 1)),
    ];
    for (hit, dx, dy) in corners {
        if hit && dx * dx + dy * dy > r * r {
            return false;
        }
    }
    true
}

fn hline(img: &mut RgbImage, x0: i32, x1: i32, y: i32, color: Rgb<u8>) {
    let (a, b) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
    for x in a..=b {
        put(img, x, y, color);
    }
}

fn blend_u8(c: u8, d: u8, a: f32) -> u8 {
    ((c as f32) * (1.0 - a) + (d as f32) * a) as u8
}

fn put(img: &mut RgbImage, x: i32, y: i32, color: Rgb<u8>) {
    if x < 0 || y < 0 {
        return;
    }
    let (x, y) = (x as u32, y as u32);
    if x < img.width() && y < img.height() {
        img.put_pixel(x, y, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::types::Node;

    #[test]
    fn udp_column_shows_unknown_not_hidden() {
        let mut n = Node::default();
        n.nat_type = "Unknown".into();
        assert!(should_show_udp_col(&n));
        assert_eq!(udp_label("Unknown"), "未知");
        n.nat_type = "Blocked".into();
        assert!(should_show_udp_col(&n));
        assert_eq!(udp_label("Blocked"), "不支持");
        n.nat_type.clear();
        assert!(!should_show_udp_col(&n));
    }

    #[test]
    fn tls_footer_three_states() {
        assert!(tls_footer_state(5, 0, 0).is_none());
        let all_ok = tls_footer_state(5, 5, 0).expect("all ok");
        assert!(matches!(all_ok.0, TlsFootKind::AllOk));
        assert_eq!(all_ok.1, "已核实全部节点的 TLS 证书");
        let partial = tls_footer_state(5, 2, 1).expect("partial");
        assert!(matches!(partial.0, TlsFootKind::Partial));
        assert_eq!(partial.1, "已核实部分节点的 TLS 证书");
        let all_bad = tls_footer_state(5, 0, 5).expect("all bad");
        assert!(matches!(all_bad.0, TlsFootKind::AllBad));
        assert_eq!(all_bad.1, "未核实全部节点的 TLS 证书");
    }

    #[test]
    fn render_compact_png_smoke() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let work = root.join("engine");
        let font = work
            .join("tools")
            .join("misc")
            .join("SourceHanSansCN-Medium.otf");
        if !font.exists() {
            eprintln!("skip png smoke: font missing at {}", font.display());
            return;
        }
        let mut nodes = Vec::new();
        for i in 0..6 {
            let mut n = Node::default();
            n.id = i;
            n.remarks = format!("🇷🇺 测试节点 {i}");
            n.outbound_geo.country_code = "RU".into();
            n.proxy_type = if i % 2 == 0 { "VLESS" } else { "Hysteria2" }.into();
            n.site_ping_ms = 80.0 + i as f64 * 10.0;
            n.avg_ping_ms = n.site_ping_ms;
            n.avg_speed = "12.3MB".into();
            n.max_speed = "20.1MB".into();
            n.online = true;
            n.total_recv_bytes = 1_000_000 * (i as u64 + 1);
            n.nat_type = "FullCone".into();
            n.tls_verified = "Verified".into();
            n.raw_speed = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0]
                .map(|x| x * 1_000_000);
            nodes.push(n);
        }
        let meta = ExportMeta {
            region_line: "中国 广东 深圳".into(),
            isp: "中国电信".into(),
            test_time: "2026-07-22 05:11:00 (CST)".into(),
            duration: "00:07:58".into(),
        };
        let out = std::env::temp_dir().join("node-speedtest-export-smoke.png");
        write_png(&out, &work, &nodes, "Node Speedtest", "v1.19.29", &meta)
            .expect("write_png");
        let meta_f = std::fs::metadata(&out).expect("png meta");
        assert!(meta_f.len() > 8_000, "png too small: {}", meta_f.len());
        let img = image::open(&out).expect("open png");
        assert!(img.width() > 400);
        assert!(img.height() > 200);
        // 紧凑：6 行不应过高（相对旧版 ~36*scale 行高）
        assert!(
            img.height() < 900 * 2,
            "unexpected tall image: {}",
            img.height()
        );
        eprintln!(
            "png smoke ok: {}x{} {} bytes -> {}",
            img.width(),
            img.height(),
            meta_f.len(),
            out.display()
        );
    }
}
