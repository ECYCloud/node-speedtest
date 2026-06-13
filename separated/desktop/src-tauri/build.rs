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

    // 把仓库根的 LICENSE / NOTICE / licenses/ 复制到 src-tauri/ 下,供 tauri.conf.json
    // 的 bundle.resources 引用。Tauri 不允许 resources 走 ../.. 跳出 src-tauri,
    // 所以这里在编译时同步一次,git 通过 .gitignore 忽略副本,源唯一在仓库根。
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repo root");
    let tauri_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for name in ["LICENSE", "NOTICE"] {
        let src = repo_root.join(name);
        let dst = tauri_dir.join(name);
        if src.exists() {
            std::fs::copy(&src, &dst).unwrap_or_else(|e| {
                panic!("复制 {} 到 src-tauri 失败: {e}", src.display())
            });
            println!("cargo:rerun-if-changed={}", src.display());
        }
    }
    // 第三方依赖许可证(如 mihomo 的 GPL-3.0)放在 licenses/ 子目录下,整目录同步
    let licenses_src = repo_root.join("licenses");
    let licenses_dst = tauri_dir.join("licenses");
    if licenses_src.is_dir() {
        std::fs::create_dir_all(&licenses_dst).ok();
        for entry in std::fs::read_dir(&licenses_src).expect("read licenses/") {
            let entry = entry.expect("dir entry");
            if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                let from = entry.path();
                let to = licenses_dst.join(entry.file_name());
                std::fs::copy(&from, &to).unwrap_or_else(|e| {
                    panic!("复制 {} 失败: {e}", from.display())
                });
                println!("cargo:rerun-if-changed={}", from.display());
            }
        }
    }

    tauri_build::build()
}
