//! 运行日志：追加写入 `engine/logs/runtime.log`，供「运行日志」页查看。

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static LOG_LOCK: Mutex<()> = Mutex::new(());

fn now_str() -> String {
    // 与 lib.rs chrono_now 同一口径（UTC+8），便于对照启动日志
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64
        + 8 * 3600;
    let days = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02}")
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    // Howard Hinnant civil_from_days（公历）
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as i64, d as i64)
}

/// 写一行运行日志。`level` 建议 INFO / WARN / ERROR。
pub fn write(work_dir: &Path, level: &str, msg: impl AsRef<str>) {
    let logs = work_dir.join("logs");
    let _ = std::fs::create_dir_all(&logs);
    let path = logs.join("runtime.log");
    let line = format!("[{}] [{}] {}\n", now_str(), level, msg.as_ref());
    let _guard = LOG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
        let _ = f.flush();
    }
    eprint!("{line}");
}

pub fn info(work_dir: &Path, msg: impl AsRef<str>) {
    write(work_dir, "INFO", msg);
}

pub fn warn(work_dir: &Path, msg: impl AsRef<str>) {
    write(work_dir, "WARN", msg);
}

pub fn error(work_dir: &Path, msg: impl AsRef<str>) {
    write(work_dir, "ERROR", msg);
}
