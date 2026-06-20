// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    install_panic_hook();
    stair_speedtest_lib::run()
}

/// 启动前装 panic hook,把崩溃写到独立日志，即使 setup 之前(GTK/webkit 初始化阶段)
/// 炸了也能从磁盘上看到现场。路径用 OS 标准用户级目录，无需 AppHandle。
fn install_panic_hook() {
    let log_path = panic_log_path();
    std::panic::set_hook(Box::new(move |info| {
        eprintln!("{info}");
        let Some(p) = &log_path else { return };
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        use std::io::Write;
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let bt = std::backtrace::Backtrace::force_capture();
        let msg = format!("[{secs}] panic: {info}\nbacktrace:\n{bt}\n\n");
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
            let _ = f.write_all(msg.as_bytes());
        }
    }));
}

fn panic_log_path() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "linux")]
    {
        let home = std::env::var_os("HOME")?;
        return Some(
            std::path::PathBuf::from(home)
                .join(".local/share/com.stairspeedtest.desktop/panic.log"),
        );
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")?;
        return Some(
            std::path::PathBuf::from(home)
                .join("Library/Application Support/com.stairspeedtest.desktop/panic.log"),
        );
    }
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var_os("APPDATA")?;
        return Some(
            std::path::PathBuf::from(appdata).join("com.stairspeedtest.desktop\\panic.log"),
        );
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    None
}
