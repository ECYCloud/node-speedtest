fn main() {
    // 注入构建时间到二进制,便于运行时识别"装的是哪一版"
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() + 8 * 3600) // 北京时间
        .unwrap_or(0);
    let days = (now / 86400) as i64;
    let h = (now % 86400) / 3600;
    let m = (now % 3600) / 60;
    let d = days + 719468;
    let era = if d >= 0 { d } else { d - 146096 } / 146097;
    let doe = (d - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let mo = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let yr = if mo <= 2 { y + 1 } else { y };
    println!(
        "cargo:rustc-env=BUILD_TIME={:04}-{:02}-{:02} {:02}:{:02}",
        yr, mo, day, h, m
    );
    // 让 build.rs 改动也触发后端重新链接(避免 incremental 缓存)
    println!("cargo:rerun-if-changed=src/lib.rs");

    tauri_build::build()
}
