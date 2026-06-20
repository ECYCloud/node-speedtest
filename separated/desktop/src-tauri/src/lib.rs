use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Manager, RunEvent, State};

/// supervisor 退出标志:RunEvent::Exit 时置 true,supervisor loop 见到立刻退出,
/// 避免应用关闭瞬间(主进程已经 kill 掉子进程)还触发"自动重启"把僵尸再拉起来。
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

/// supervisor 唤醒通道:前端 invoke 实际命中 reqwest send 错误(连接失败 /
/// 超时 / 连接断)时,Rust 侧调 notify_one() 把 supervisor 从 notified().await
/// 阻塞中唤醒。supervisor 整个生命周期没有定时器,**唯一驱动源就是这个事件**:
/// 用户感知到失败的同一时刻 supervisor 才动手，毫秒级响应。HTTP 4xx/5xx 不通过
/// 这里(那是后端正常拒绝，不是掉线)。
fn supervisor_wakeup() -> &'static tokio::sync::Notify {
    use std::sync::OnceLock;
    static N: OnceLock<tokio::sync::Notify> = OnceLock::new();
    N.get_or_init(tokio::sync::Notify::new)
}

/// 后端进程统一监听地址，与 stairspeedtest pref.ini 的 [webserver] 默认值保持一致
const BACKEND_URL: &str = "http://127.0.0.1:10870";

/// 跨平台可执行文件名:Windows 带 .exe,Linux/macOS 无后缀。
#[cfg(windows)]
const ENGINE_EXE: &str = "stairspeedtest.exe";
#[cfg(not(windows))]
const ENGINE_EXE: &str = "stairspeedtest";

#[cfg(windows)]
const MIHOMO_EXE: &str = "mihomo.exe";
#[cfg(not(windows))]
const MIHOMO_EXE: &str = "mihomo";

/// 持有后端子进程句柄，应用退出时统一收尾
struct BackendState {
    child: Mutex<Option<Child>>,
}

/// 路径白名单校验:确保 candidate 解析后落在 base 之内。
/// 防 ".." / 软链接逃逸 / 不存在但语法合法的恶意路径。
/// 任何失败都返回统一 "拒绝访问" 文案，不向调用方泄露真实磁盘布局。
fn path_within(base: &Path, candidate: &Path) -> Result<PathBuf, String> {
    let real_candidate = std::fs::canonicalize(candidate)
        .map_err(|_| String::from("拒绝访问:路径无法定位"))?;
    let real_base = std::fs::canonicalize(base)
        .map_err(|_| String::from("拒绝访问:基准目录无法定位"))?;
    if !real_candidate.starts_with(&real_base) {
        return Err(String::from("拒绝访问:路径越权"));
    }
    Ok(real_candidate)
}

/// 历史记录 / 节点名等用户输入做文件名时的安全过滤。
/// 命中任一危险字符即拒绝(不做替换:替换后再拼路径仍可能越权)。
fn ensure_safe_basename(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err(String::from("名称不能为空"));
    }
    if name.contains("..")
        || name.contains('/')
        || name.contains('\\')
        || name.contains(':')
        || name.contains('\0')
    {
        return Err(String::from("名称含非法字符"));
    }
    Ok(())
}

/// 资源目录(打包时只读)。dev 走仓库内的 engine,release 走 Tauri 内置资源目录。
fn bundled_engine_dir(app: &AppHandle) -> Result<PathBuf, String> {
    #[cfg(debug_assertions)]
    {
        let _ = app;
        Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("engine"))
    }
    #[cfg(not(debug_assertions))]
    {
        app.path()
            .resource_dir()
            .map(|p| p.join("engine"))
            .map_err(|e| e.to_string())
    }
}

/// 运行时 engine 目录(用户级可写位置)。
/// 关键:Program Files 装机用户没有写权限,mihomo 内核需要写 cache.db / logs/,
/// 如果直接在打包目录里跑会被 UAC 静默拒绝，导致测试结果全 N/A 但不报错。
/// 解决:启动时把 bundled_engine_dir 同步到这里再从这里运行后端。
fn runtime_engine_dir(app: &AppHandle) -> Result<PathBuf, String> {
    #[cfg(debug_assertions)]
    {
        // dev 模式直接用仓库内的 engine(开发者机器一定可写)
        return bundled_engine_dir(app);
    }
    #[cfg(not(debug_assertions))]
    {
        app.path()
            .app_local_data_dir()
            .map(|p| p.join("engine"))
            .map_err(|e| e.to_string())
    }
}

/// 历史记录所在目录(运行时可写)
fn engine_dir(app: &AppHandle) -> Result<PathBuf, String> {
    runtime_engine_dir(app)
}

/// 启动前同步 bundled engine 到 runtime engine。
/// 仅在 release 模式下执行;dev 模式 runtime = bundled,跳过。
///
/// 升级判定 + 完整性校验:
///   1. 主程序指纹(stairspeedtest.exe 的 size+mtime)— 升级时触发同步
///   2. tools/ 关键 sentinel 文件 — 历史问题:某次首次安装 copy_dir 中途
///      失败，只复制了根目录的 DLL/exe,tools/ 子树整个丢了，但 exe 指纹依然
///      "看起来匹配",于是后续所有启动都 skip,导致 mihomo 永远拉不起来 +
///      字体加载失败 + 测试结果全 0/N/A。
///      现在改为只要任意一个 sentinel 缺失就强制重新同步。
///
/// 用户的 logs/ 与 results/(历史记录、测试日志)始终保留，只覆盖 exe / DLL /
/// tools / pref.ini 这些只读资产。
fn ensure_runtime_engine(app: &AppHandle) -> Result<(), String> {
    #[cfg(debug_assertions)]
    {
        let _ = app;
        return Ok(());
    }
    #[cfg(not(debug_assertions))]
    {
        let src = bundled_engine_dir(app)?;
        let dst = runtime_engine_dir(app)?;
        let src_exe = src.join(ENGINE_EXE);

        // 关键资产 sentinel:任意一个缺失视作 runtime 不完整，必须重同步。
        // 涵盖 mihomo / 字体 / config / pref,以及 Linux 上引擎依赖捆进来的核心 .so —
        // 老版本 deb 没有这些 .so,新版本检测到缺失就强制全量重写。
        #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
        let mut sentinels: Vec<std::path::PathBuf> = vec![
            std::path::PathBuf::from("tools/clients").join(MIHOMO_EXE),
            std::path::PathBuf::from("tools/misc/SourceHanSansCN-Medium.otf"),
            std::path::PathBuf::from("config.yaml"),
            std::path::PathBuf::from("pref.ini"),
        ];
        #[cfg(target_os = "linux")]
        {
            sentinels.extend([
                std::path::PathBuf::from("libyaml-cpp.so.0.7"),
                std::path::PathBuf::from("libpcre2-8.so.0"),
                std::path::PathBuf::from("libcurl.so.4"),
            ]);
        }

        // 版本指纹:CARGO_PKG_VERSION + bundled stairspeedtest 的 (size, mtime)。
        // 同 tag 重发因 mtime 变化也会触发重同步;Tauri std::fs::copy 不保留 mtime,
        // 用文件持久化比"对比 dst exe mtime"更可靠。
        let want_sig = compute_engine_signature(&src_exe);
        let sig_path = dst.join(".runtime_fingerprint");
        let have_sig = std::fs::read_to_string(&sig_path).ok();

        let runtime_complete = sentinels.iter().all(|rel| dst.join(rel).exists());
        if runtime_complete && have_sig.as_deref() == Some(want_sig.as_str()) {
            return Ok(());
        }

        std::fs::create_dir_all(&dst).map_err(|e| format!("创建运行时目录失败: {e}"))?;
        // 跳过用户数据 logs/results,其他全量覆盖
        copy_dir_excluding(&src, &dst, &["logs", "results"])?;
        let _ = std::fs::create_dir_all(dst.join("logs"));
        let _ = std::fs::create_dir_all(dst.join("results"));
        let _ = std::fs::write(&sig_path, &want_sig);
        Ok(())
    }
}

/// 计算 bundled stairspeedtest 主程序的版本指纹，用作 runtime engine 是否需要重同步的判定。
#[cfg(not(debug_assertions))]
fn compute_engine_signature(src_exe: &std::path::Path) -> String {
    let (size, mtime) = match std::fs::metadata(src_exe) {
        Ok(m) => {
            let t = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            (m.len(), t)
        }
        Err(_) => (0, 0),
    };
    format!(
        "v{} size={} mtime={}",
        env!("CARGO_PKG_VERSION"),
        size,
        mtime
    )
}

/// 递归复制目录，跳过指定子目录(按基名)
#[cfg_attr(debug_assertions, allow(dead_code))]
fn copy_dir_excluding(src: &Path, dst: &Path, exclude: &[&str]) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("mkdir {}: {e}", dst.display()))?;
    for entry in std::fs::read_dir(src).map_err(|e| format!("read_dir {}: {e}", src.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name_os = entry.file_name();
        let name = name_os.to_string_lossy();
        if exclude.iter().any(|x| *x == name.as_ref()) {
            continue;
        }
        let s = entry.path();
        let d = dst.join(&*name);
        let ft = entry.file_type().map_err(|e| e.to_string())?;
        if ft.is_dir() {
            copy_dir_excluding(&s, &d, exclude)?;
        } else {
            std::fs::copy(&s, &d)
                .map_err(|e| format!("copy {} -> {}: {e}", s.display(), d.display()))?;
        }
    }
    Ok(())
}

/// 启动后端 stairspeedtest.exe /web,并把工作目录设到 engine 内,
/// 这样它能就近找到 DLL、tools/、pref.ini、config.yaml。
fn spawn_backend(app: &AppHandle) -> Result<Child, String> {
    let dir = engine_dir(app)?;
    let exe = dir.join(ENGINE_EXE);
    if !exe.exists() {
        return Err(format!("后端可执行文件不存在: {}", exe.display()));
    }

    // stderr/stdout 写入日志文件，便于排查 mihomo 启动失败等问题。
    // 这比 Stdio::null() 更友好——出问题能直接看日志，而不是黑盒。
    let logs_dir = dir.join("logs");
    let _ = std::fs::create_dir_all(&logs_dir);
    let stdout_log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs_dir.join("sidecar-stdout.log"))
        .map_err(|e| format!("打开 stdout 日志失败: {e}"))?;
    let stderr_log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs_dir.join("sidecar-stderr.log"))
        .map_err(|e| format!("打开 stderr 日志失败: {e}"))?;

    let mut cmd = Command::new(&exe);
    cmd.arg("/web")
        .current_dir(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_log))
        .stderr(Stdio::from(stderr_log));

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // 关键修复:不用 CREATE_NO_WINDOW(它会让 mihomo 内核在某些 IO 路径上行为异常,
        // 导致测试结果全 N/A)。改用 DETACHED_PROCESS:子进程脱离父级控制台,
        // 自己有完整的控制台句柄(虽然不可见),mihomo 行为与 cmd 直接启动一致。
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }

    cmd.spawn().map_err(|e| format!("启动后端失败: {e}"))
}

/// sidecar supervisor:**纯事件驱动**,后端被前端感知到离线时立即重启。
///
/// 根因:此前 spawn_backend 后没人监督，后端一旦崩溃(rapidjson assert / 内核段错误 /
/// mihomo 死锁 / libevent 阻塞)前端 Status 就永远红下去，用户必须自己点"设置 → 重启后端"。
/// 现在做成自愈:用户感知到失败的同一时刻 supervisor 已经在重启了。
///
/// 工作模型:
///   - 阻塞在 `supervisor_wakeup().notified()` 上等事件,**没有任何定时器在转**
///   - 唤醒源:api_get / api_post_json / api_post_file 在 reqwest send/text 错误时
///     调 `notify_one()`(网络层失败，非业务 4xx/5xx)
///   - 唤醒后做一次确认探活防止单次抖动误杀:
///     - 进程已退出 → 直接重启
///     - 进程在 + /getversion 探活成功 → 偶发抖动，后端其实健康，本轮不动
///     - 进程在 + 探活失败 → 端口在但内核僵化(死锁 / hang),kill 后重启
///
/// 与 restart_backend 共用 BackendState.child 这把锁:supervisor 持锁判断 + 持锁 spawn,
/// restart_backend 也持锁到 spawn 完成，二者不会 race 出"中间态 None → 误重启"。
async fn supervisor_loop(handle: AppHandle) {
    use std::time::{Duration, Instant};
    // setup 流程先 spawn_backend 再 spawn(supervisor_loop),supervisor 进 loop 时
    // 进程已经在跑，无需启动缓冲;直接进入事件等待。
    let mut last_restart_at = Instant::now() - Duration::from_secs(60);
    // 连续重启失败计数:每 spawn_backend 失败一次 +1，成功清零。
    // 用作指数退避底数，让"端口持续被占 / exe 丢失 / 磁盘满"等长期失败
    // 不再每次 notify 醒来都立刻重试，避免日志/系统调用爆炸。
    let mut consecutive_failures: u32 = 0;
    loop {
        if SHUTTING_DOWN.load(Ordering::SeqCst) {
            return;
        }
        // 阻塞等事件:前端 reqwest 失败时 notify_one() 唤醒。没有任何定时器,
        // 没人触发就一直等，体感"用户感知失败 → supervisor 同一时刻动手"。
        supervisor_wakeup().notified().await;
        if SHUTTING_DOWN.load(Ordering::SeqCst) {
            return;
        }

        // 第一步:持锁判断进程状态,Mutex 不是 async 的所以 await 必须放锁外
        enum ProcessState {
            Alive,        // 进程在跑，继续做 HTTP 确认探活
            Exited,       // try_wait 拿到 status / 出错 / state 是 None,直接重启
        }
        let proc_state = {
            let state = handle.state::<BackendState>();
            let mut guard = match state.child.lock() {
                Ok(g) => g,
                Err(_) => continue,
            };
            match guard.as_mut() {
                Some(child) => match child.try_wait() {
                    Ok(Some(status)) => {
                        eprintln!("[supervisor] 后端进程已退出 (status={status:?}),将重启");
                        *guard = None;
                        ProcessState::Exited
                    }
                    Ok(None) => ProcessState::Alive,
                    Err(e) => {
                        eprintln!("[supervisor] try_wait 出错: {e},按崩溃处理");
                        *guard = None;
                        ProcessState::Exited
                    }
                },
                // None 表示 setup 时 spawn 失败、或 restart_backend 异常路径,
                // 也补一次自动起
                None => ProcessState::Exited,
            }
        };

        // 第二步:进程还在的话做一次确认探活;失败即判定 hang,主动 kill 走重启路径。
        // 单次定生死 — 触发源是用户实际请求失败，信任度高，不再额外累计计数。
        let need_respawn = match proc_state {
            ProcessState::Exited => true,
            ProcessState::Alive => {
                if check_backend_alive().await {
                    // 后端其实健康，刚才那次失败是偶发抖动，不动。顺手把失败计数清零:
                    // 后端能响应 = 系统已恢复，下次再失败应当从基础 3s 防抖重新算。
                    consecutive_failures = 0;
                    false
                } else {
                    eprintln!("[supervisor] /getversion 探活失败，后端僵化，主动 kill 进入重启");
                    let state = handle.state::<BackendState>();
                    if let Ok(mut guard) = state.child.lock() {
                        if let Some(mut c) = guard.take() {
                            kill_backend_tree(&mut c);
                        }
                    }
                    true
                }
            }
        };

        if !need_respawn {
            continue;
        }

        // 退避:基础 3s + 指数(2^n × 3s 封顶 60s)。第 0/1 次失败用 3s 基础防抖；
        // 后续每多一次失败就翻倍延迟。封顶 60s 让长期错误不至于完全静默。
        // 不调 sleep — Notify 不累积，前端持续失败只会让 supervisor 反复
        // "醒来 → 比较时间 → continue"，毫秒级 noop。
        let backoff_secs: u64 = match consecutive_failures {
            0 | 1 => 3,
            n => std::cmp::min(60u64, 3u64 << std::cmp::min(n - 1, 5)),
        };
        if Instant::now().duration_since(last_restart_at) < Duration::from_secs(backoff_secs) {
            continue;
        }
        last_restart_at = Instant::now();

        cleanup_orphans();
        if let Err(e) = ensure_runtime_engine(&handle) {
            eprintln!("[supervisor] ensure_runtime_engine 失败: {e}");
            consecutive_failures = consecutive_failures.saturating_add(1);
            continue;
        }
        // 持锁 spawn,避免 supervisor 自己下一轮见到 None 又触发一次
        let state = handle.state::<BackendState>();
        let mut guard = match state.child.lock() {
            Ok(g) => g,
            Err(_) => continue,
        };
        match spawn_backend(&handle) {
            Ok(c) => {
                *guard = Some(c);
                consecutive_failures = 0;
                eprintln!("[supervisor] 后端已自动重启");
            }
            Err(e) => {
                consecutive_failures = consecutive_failures.saturating_add(1);
                eprintln!(
                    "[supervisor] 自动重启失败(连续 {consecutive_failures} 次，下次退避 {}s): {e}",
                    match consecutive_failures {
                        0 | 1 => 3,
                        n => std::cmp::min(60u64, 3u64 << std::cmp::min(n - 1, 5)),
                    }
                );
            }
        }
    }
}

/// HTTP 探活:GET /getversion,3s 内拿到 2xx 视为健康。3s 上限大于普通响应耗时
/// 一个数量级，既不会被偶发抖动误判，也能在内核 hang 时尽快触发重启。
async fn check_backend_alive() -> bool {
    use std::time::Duration;
    let fut = local_backend_client()
        .get(format!("{}/getversion", BACKEND_URL))
        .send();
    match tokio::time::timeout(Duration::from_secs(3), fut).await {
        Ok(Ok(r)) => r.status().is_success(),
        _ => false,
    }
}

/// 在 Windows 上调用 taskkill / 其他外部命令，加 CREATE_NO_WINDOW 防止闪现 cmd 窗口
#[cfg(windows)]
fn silent_command(program: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let mut cmd = Command::new(program);
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd
}

/// 启动后端前先清理可能残留的孤儿 mihomo / stairspeedtest 进程,
/// 避免端口/句柄冲突导致新后端启动失败或测试中途异常退出。
#[cfg(windows)]
fn cleanup_orphans() {
    for image in ["mihomo.exe", "stairspeedtest.exe"] {
        let _ = silent_command("taskkill")
            .args(["/F", "/IM", image, "/T"])
            .status();
    }
}

/// Linux/macOS 走 pkill -f,匹配命令路径(sidecar 用绝对路径起 mihomo)。
#[cfg(not(windows))]
fn cleanup_orphans() {
    for name in ["mihomo", "stairspeedtest"] {
        let _ = Command::new("pkill")
            .args(["-f", name])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

// ===== 任务栏图标高分辨率修复 =====
//
// 现象:应用启动几秒后，任务栏图标突然变模糊。
//
// 根因:Tauri 2.11.2 的 tauri-codegen 在编译期解析 bundle.icon 时，只读取 ICO 文件的
// 第一个条目(`entries()[0]`)作为运行时窗口图标 RGBA 缓冲(官方 issue #14596 /
// PR #15241,2.11.2 未修)。我们的 icon.ico 按惯例从小到大排列，第一个条目是 16x16,
// 被 wry 通过 WM_SETICON 设给主窗口后，任务栏(需要 32/48 px)就只能从 16x16 上采样,
// Alt-Tab、任务栏预览全部被替换成糊图。EXE 嵌入资源里的多分辨率 ICO 在窗口创建前
// 一直被系统使用，所以"刚启动还清晰、过几秒变糊"。
//
// 修复:绕过 Tauri 内部那张 16x16 RGBA,在 setup 拿到主窗口 HWND 后，自己用 Win32
// API 从完整的 ICO 字节里挑出最匹配 ICON_BIG/ICON_SMALL 期望尺寸的条目,
// CreateIconFromResourceEx 创建 HICON,再 WM_SETICON 覆盖。这样:
//   - ICON_BIG  ← 距离 SM_CXICON  最近的条目(任务栏 / Alt-Tab 用)
//   - ICON_SMALL ← 距离 SM_CXSMICON 最近的条目(标题栏 / 任务管理器用)
#[cfg(windows)]
const APP_ICO_BYTES: &[u8] = include_bytes!("../icons/icon.ico");

/// 在 ICO 字节流中按目标像素尺寸挑选最匹配的条目，返回其原始位图数据切片(可直接交给
/// CreateIconFromResourceEx)。距离相同时偏好更大的条目，避免在小屏幕上被选到 16x16。
#[cfg(windows)]
fn pick_ico_entry(ico: &[u8], want: i32) -> Option<&[u8]> {
    if ico.len() < 6 {
        return None;
    }
    // ICONDIR: reserved(2) + type(2) + count(2)
    let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
    let dir_end = 6usize.checked_add(count.checked_mul(16)?)?;
    if ico.len() < dir_end {
        return None;
    }
    let mut best: Option<(i32, i32, usize, usize)> = None; // (-dist, side_sum, off, size)
    for i in 0..count {
        let o = 6 + i * 16;
        // ICONDIRENTRY 中 0 表示 256
        let w = if ico[o] == 0 { 256 } else { ico[o] as i32 };
        let h = if ico[o + 1] == 0 { 256 } else { ico[o + 1] as i32 };
        let size = u32::from_le_bytes([ico[o + 8], ico[o + 9], ico[o + 10], ico[o + 11]]) as usize;
        let off = u32::from_le_bytes([ico[o + 12], ico[o + 13], ico[o + 14], ico[o + 15]]) as usize;
        if off.checked_add(size).map_or(true, |end| end > ico.len()) {
            continue;
        }
        let dist = (w - want).abs() + (h - want).abs();
        let side = w + h;
        // 排序 key:先按距离最小，再按尺寸最大(同距离取大，例如 want=32 时 32 优于 16)
        let key = (-dist, side);
        if best.map_or(true, |(d, s, _, _)| (key.0, key.1) > (d, s)) {
            best = Some((key.0, key.1, off, size));
        }
    }
    best.map(|(_, _, off, size)| &ico[off..off + size])
}

/// 给指定 HWND 重设 ICON_BIG / ICON_SMALL 为高分辨率版本。
/// 注意:WM_SETICON 设置后图标的销毁由窗口生命周期负责，这里设完不能 DestroyIcon
/// (否则系统再次绘制时拿到悬空句柄会画出空白/黑块)。
#[cfg(windows)]
fn apply_high_res_icons(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateIconFromResourceEx, GetSystemMetrics, SendMessageW, ICON_BIG, ICON_SMALL,
        LR_DEFAULTCOLOR, SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, WM_SETICON,
    };

    // Tauri 依赖的 windows 0.61 中 HWND 是 *mut c_void 的 newtype,与 windows-sys 0.59
    // 的 HWND(同样是 *mut c_void)二进制布局一致，直接 as 转即可。
    let hwnd: HWND = match window.hwnd() {
        Ok(h) => h.0 as HWND,
        Err(e) => {
            eprintln!("[icon] 获取主窗口 HWND 失败: {e}");
            return;
        }
    };

    unsafe {
        // ICON_BIG: 任务栏 / Alt-Tab。SM_CXICON 在 100% DPI 下是 32,150% 是 48。
        let want_big = GetSystemMetrics(SM_CXICON)
            .max(GetSystemMetrics(SM_CYICON))
            .max(32);
        if let Some(entry) = pick_ico_entry(APP_ICO_BYTES, want_big) {
            // 第三参数 fIcon=1 表示创建 icon(不是 cursor),
            // 第四参数 dwVer=0x00030000 是 Windows 3.0+ 标准格式标记，固定值。
            let h_big = CreateIconFromResourceEx(
                entry.as_ptr(),
                entry.len() as u32,
                1, // TRUE = icon
                0x0003_0000,
                want_big,
                want_big,
                LR_DEFAULTCOLOR,
            );
            if !h_big.is_null() {
                SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, h_big as isize);
            } else {
                eprintln!("[icon] CreateIconFromResourceEx(big) 返回 NULL");
            }
        }

        // ICON_SMALL: 标题栏 / 任务管理器子窗口列表。SM_CXSMICON 通常 16/20/24。
        let want_small = GetSystemMetrics(SM_CXSMICON)
            .max(GetSystemMetrics(SM_CYSMICON))
            .max(16);
        if let Some(entry) = pick_ico_entry(APP_ICO_BYTES, want_small) {
            let h_small = CreateIconFromResourceEx(
                entry.as_ptr(),
                entry.len() as u32,
                1,
                0x0003_0000,
                want_small,
                want_small,
                LR_DEFAULTCOLOR,
            );
            if !h_small.is_null() {
                SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, h_small as isize);
            } else {
                eprintln!("[icon] CreateIconFromResourceEx(small) 返回 NULL");
            }
        }
    }
}

#[cfg(not(windows))]
fn apply_high_res_icons(_window: &tauri::WebviewWindow) {}
// ===== 任务栏图标高分辨率修复 end =====

/// 彻底清理后端进程及其子孙(mihomo 内核),避免孤儿。child.kill() 不递归,
/// Windows 用 taskkill /T,Linux/macOS 用 pkill -P + pkill -f mihomo 兜底。
fn kill_backend_tree(child: &mut Child) {
    let pid = child.id();
    let _ = child.kill();
    let _ = child.wait();
    #[cfg(windows)]
    {
        // /T 杀整个进程树,/F 强制
        let _ = silent_command("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .status();
        // 同时把名为 mihomo.exe 的孤儿也清理一遍(以防万一)
        let _ = silent_command("taskkill")
            .args(["/F", "/IM", "mihomo.exe", "/T"])
            .status();
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("pkill")
            .args(["-P", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = Command::new("pkill")
            .args(["-f", "mihomo"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

#[tauri::command]
fn backend_url() -> &'static str {
    BACKEND_URL
}

/// 启动闪烁修复:tauri.conf.json 设了 visible:false,窗口启动时不可见,
/// 等前端 React 完成首屏渲染后调这个命令把主窗口显示出来 + 设置焦点。
/// 这样用户看到的第一帧就是渲染好的 splash / 主界面，不再经历"白屏 → 紫屏"的闪烁。
///
/// 同时在这里再调一次 apply_high_res_icons —— 修复 Tauri runtime 注入 16x16 RGBA
/// 导致任务栏图标模糊的问题(详见 apply_high_res_icons 上方注释)。setup 已经设过
/// 一次，这里再设一次是为了覆盖某些时序下 wry 可能在窗口 show 之前重新写入低分图标
/// 的极端情况，确保用户看到的第一帧任务栏图标就是高分辨率版本。
#[tauri::command]
fn show_main_window(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        apply_high_res_icons(&w);
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// 与后端 127.0.0.1 通信的专用 reqwest 客户端。
/// 关键:**.no_proxy()** 绕过系统代理。用户开了系统代理(HTTP_PROXY)时,
/// reqwest 默认会通过代理转发请求，但代理通常不允许中转 127.0.0.1,
/// 导致 POST /start、POST /readsubscriptions 全失败 → 测试全 N/A。
///
/// 稳定性参数:
///   - connect_timeout(2s):127.0.0.1 偶发 SYN 失败时立刻报错，而不是吊死
///     在整体 timeout 上让前端 Status 红屏几十秒
///   - pool_idle_timeout(20s) + tcp_keepalive(15s):后端 keep-alive 开启后
///     复用连接，避免短连接洪流让 Windows ephemeral port 紧张
///   - timeout(15s):轻量请求(/getversion /status /getresults /stop)预算
///     绰绰有余;重型请求(/start /readsubscriptions /readfileconfig)走
///     long_backend_client(60s)。
fn local_backend_client() -> &'static reqwest::Client {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(std::time::Duration::from_secs(2))
            .pool_idle_timeout(std::time::Duration::from_secs(20))
            .tcp_keepalive(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .expect("无法创建 local backend client")
    })
}

/// 重型请求专用:/start 内部要等 mihomo 重启就绪、订阅下载、解析等,
/// 单次 15s 预算不够。其他参数与 local_backend_client 一致。
fn long_backend_client() -> &'static reqwest::Client {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(std::time::Duration::from_secs(2))
            .pool_idle_timeout(std::time::Duration::from_secs(20))
            .tcp_keepalive(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .expect("无法创建 long backend client")
    })
}

/// 路径白名单:重型请求走 60s timeout,其余走 15s。
fn pick_client(path: &str) -> &'static reqwest::Client {
    if path.starts_with("/readsubscriptions")
        || path.starts_with("/readfileconfig")
        || path.starts_with("/start")
    {
        long_backend_client()
    } else {
        local_backend_client()
    }
}

/// 通过 Rust 侧 reqwest 代理向后端发起请求，绕过 webview 的 mixed-content / CORS 限制
/// 以及系统代理拦截 127.0.0.1 的问题。统一走 invoke + .no_proxy() 是最稳的做法。
///
/// 网络层失败(reqwest::Error,非 HTTP 业务码)会通过 supervisor_wakeup 唤醒
/// supervisor 立即做确认探活并按需重启。这是 supervisor 的唯一驱动源 —
/// 用户感知到失败的同一时刻 supervisor 才动手，没有任何后台定时器在转。
#[tauri::command]
async fn api_get(path: String) -> Result<String, String> {
    let url = format!("{}{}", BACKEND_URL, path);
    let res = pick_client(&path)
        .get(&url)
        .send()
        .await
        .map_err(|e| {
            supervisor_wakeup().notify_one();
            e.to_string()
        })?;
    let status = res.status();
    let text = res.text().await.map_err(|e| {
        supervisor_wakeup().notify_one();
        e.to_string()
    })?;
    if !status.is_success() {
        return Err(format!("{} {}: {}", path, status.as_u16(), text));
    }
    Ok(text)
}

#[tauri::command]
async fn api_post_json(path: String, body: String) -> Result<String, String> {
    let url = format!("{}{}", BACKEND_URL, path);
    let res = pick_client(&path)
        .post(&url)
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|e| {
            supervisor_wakeup().notify_one();
            e.to_string()
        })?;
    let status = res.status();
    let text = res.text().await.map_err(|e| {
        supervisor_wakeup().notify_one();
        e.to_string()
    })?;
    if !status.is_success() {
        return Err(format!("{} {}: {}", path, status.as_u16(), text));
    }
    Ok(text)
}

/// 上传本地配置文件:用 multipart/form-data,字段名 file
#[tauri::command]
async fn api_post_file(
    path: String,
    file_name: String,
    file_bytes: Vec<u8>,
) -> Result<String, String> {
    let url = format!("{}{}", BACKEND_URL, path);
    let part = reqwest::multipart::Part::bytes(file_bytes).file_name(file_name);
    let form = reqwest::multipart::Form::new().part("file", part);
    let res = pick_client(&path)
        .post(&url)
        .multipart(form)
        .send()
        .await
        .map_err(|e| {
            supervisor_wakeup().notify_one();
            e.to_string()
        })?;
    let status = res.status();
    let text = res.text().await.map_err(|e| {
        supervisor_wakeup().notify_one();
        e.to_string()
    })?;
    if !status.is_success() {
        return Err(format!("{} {}: {}", path, status.as_u16(), text));
    }
    Ok(text)
}

/// 选定本地配置文件后，由 Rust 端一步完成:读取文件字节 → multipart 上传到后端
/// /readfileconfig → 返回后端响应文本(节点 JSON 数组，或 "error"/"running")。
///
/// 为什么不在前端做:Tauri webview 打包模式下 JS 侧读文件 + 多次 invoke 往返
/// (read_file_base64 → atob → api_post_file)任一环出错都难以察觉，且经过最
/// 脆弱的链路。收敛到 Rust 单命令后，文件读取与上传全程可控，任何失败都通过
/// Result::Err 冒泡到前端显示，不再"点了没反应"。
#[tauri::command]
async fn import_config_file(path: String) -> Result<String, String> {
    // 来自系统打开对话框的路径不限基准目录(用户主动指认的文件)，但加 10 MB 上限
    // 防止误选/恶意大文件把后端 multipart 解析撑爆。订阅与 yaml 配置正常都 < 1 MB。
    const MAX_BYTES: u64 = 10 * 1024 * 1024;
    let p = Path::new(&path);
    if !p.is_file() {
        return Err(format!("文件不存在: {path}"));
    }
    if let Ok(meta) = std::fs::metadata(p) {
        if meta.len() > MAX_BYTES {
            return Err(format!("文件过大({} 字节，上限 10 MB)", meta.len()));
        }
    }
    let bytes = std::fs::read(p).map_err(|e| format!("读取文件失败 {path}: {e}"))?;
    let file_name = p
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("config")
        .to_string();
    let url = format!("{}/readfileconfig", BACKEND_URL);
    let part = reqwest::multipart::Part::bytes(bytes).file_name(file_name);
    let form = reqwest::multipart::Form::new().part("file", part);
    let res = local_backend_client()
        .post(&url)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("上传到后端失败: {e}"))?;
    let status = res.status();
    let text = res.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("/readfileconfig {}: {}", status.as_u16(), text));
    }
    Ok(text)
}

#[tauri::command]
fn restart_backend(app: AppHandle, state: State<BackendState>) -> Result<(), String> {
    // 全程持锁:take 旧 child → spawn 新 child → 写回锁，中间不释放。
    // 关键:supervisor loop 也走同一把锁，这里持锁 spawn 能保证 supervisor 看到的
    // 永远是"完整状态"(要么旧 Some、要么新 Some),不会撞到中间的 None 误以为
    // 后端崩了再触发一次重启。
    let mut guard = state.child.lock().map_err(|e| e.to_string())?;
    if let Some(mut c) = guard.take() {
        kill_backend_tree(&mut c);
    }
    // 给 OS 一点时间清理端口/句柄
    std::thread::sleep(std::time::Duration::from_millis(400));
    // 重启前先做完整性检查:如果用户的 runtime engine 缺资产(tools/ 子树丢失等),
    // 这里会触发重新同步，从而让"设置 → 重启后端"成为通用自愈入口。
    ensure_runtime_engine(&app)?;
    let new_child = spawn_backend(&app)?;
    *guard = Some(new_child);
    Ok(())
}

/// clear_app_data 返回结构:report 里把删了哪些路径告诉前端用于展示,
/// 即使 best-effort 删除遇到部分占用失败，也用 errors 把跳过的项汇总。
#[derive(Serialize)]
struct ClearAppDataResult {
    cleared: Vec<String>,
    errors: Vec<String>,
}

/// 删除"应用程序数据":与 NSIS 卸载向导的"删除应用程序数据"勾选项语义对齐,
/// 但范围收敛到我们自己写入的 engine 子目录(测速结果 / 日志 / 订阅记录 / mihomo
/// 缓存 / pref.ini),webview 缓存被当前进程占用删不掉，留给 OS 在退出后释放。
///
/// 流程:
///   1. 停掉后端 child + mihomo 内核，释放对 engine/ 下文件的占用句柄
///   2. 关闭 supervisor 自动重启(SHUTTING_DOWN 置位),避免删完目录又被拉起来
///   3. best-effort 递归删除 runtime_engine_dir 与 panic.log
///   4. 命令返回后由 main 端短暂延迟再调 app.restart() 重启应用,
///      下次启动 ensure_runtime_engine 会从 bundle 重建 engine
///
/// 用 app.restart() 而不是前端 close():后者要 plugin:window|close 的 ACL,
/// 默认 capability 没授权会被拒(用户实际看到 "Command plugin:window|close
/// not allowed by ACL")。app.restart() 走 Tauri Manager 直通，不受 ACL 影响。
///
/// dev 模式直接拒绝:dev 下 runtime_engine_dir 指向仓库 engine/,误删后开发环境会废。
#[tauri::command]
fn clear_app_data(
    app: AppHandle,
    state: State<BackendState>,
) -> Result<ClearAppDataResult, String> {
    #[cfg(debug_assertions)]
    {
        let _ = (app, state);
        return Err("开发模式下禁止清理应用数据，以免误删仓库 engine 目录".into());
    }
    #[cfg(not(debug_assertions))]
    {
        // 1. 停后端，持锁直到 child 真正退出,supervisor 看不到 None 不会误重启
        SHUTTING_DOWN.store(true, Ordering::SeqCst);
        {
            let mut guard = state.child.lock().map_err(|e| e.to_string())?;
            if let Some(mut c) = guard.take() {
                kill_backend_tree(&mut c);
            }
        }
        // 2. 兜底再杀一遍 mihomo 孤儿，确保 cache.db / logs/ 句柄全释放
        cleanup_orphans();
        std::thread::sleep(std::time::Duration::from_millis(500));

        let mut cleared: Vec<String> = Vec::new();
        let mut errors: Vec<String> = Vec::new();

        // 3. runtime engine 目录:测速记录 / 日志 / 订阅 / mihomo 缓存 / pref.ini 都在这
        let engine = runtime_engine_dir(&app)?;
        if engine.exists() {
            match std::fs::remove_dir_all(&engine) {
                Ok(_) => cleared.push(engine.display().to_string()),
                Err(e) => errors.push(format!("{}: {e}", engine.display())),
            }
        }

        // 4. panic.log 在 %APPDATA%\com.stairspeedtest.desktop 下，与 main.rs 的
        //    panic_log_path 保持同一路径，避免历史崩溃记录残留
        if let Ok(roaming) = app.path().app_data_dir() {
            let panic_log = roaming.join("panic.log");
            if panic_log.exists() {
                match std::fs::remove_file(&panic_log) {
                    Ok(_) => cleared.push(panic_log.display().to_string()),
                    Err(e) => errors.push(format!("{}: {e}", panic_log.display())),
                }
            }
        }

        // 5. 调度重启:延迟 300ms 让本次 invoke 的响应先回到前端，然后整进程
        //    abort 拉起新实例。app.restart() 内部会发 RunEvent::Exit,
        //    supervisor 已经被 SHUTTING_DOWN 关掉不会再起 mihomo。
        let h = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            h.restart();
        });

        Ok(ClearAppDataResult { cleared, errors })
    }
}

#[derive(Serialize)]
struct HistoryItem {
    name: String,
    log_path: Option<String>,
    image_path: Option<String>,
    size: u64,
    modified_ms: i64,
}

#[tauri::command]
fn list_history(app: AppHandle) -> Result<Vec<HistoryItem>, String> {
    use std::collections::BTreeMap;
    let dir = engine_dir(&app)?.join("results");
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut grouped: BTreeMap<String, HistoryItem> = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()).map(String::from) else { continue };
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        let meta = entry.metadata().map_err(|e| e.to_string())?;
        let modified_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let item = grouped.entry(stem.clone()).or_insert(HistoryItem {
            name: stem,
            log_path: None,
            image_path: None,
            size: 0,
            modified_ms: 0,
        });
        let p = path.to_string_lossy().into_owned();
        match ext {
            "log" => item.log_path = Some(p),
            "png" => item.image_path = Some(p),
            _ => continue,
        }
        item.size += meta.len();
        if modified_ms > item.modified_ms {
            item.modified_ms = modified_ms;
        }
    }
    let mut out: Vec<HistoryItem> = grouped.into_values().collect();
    out.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms));
    Ok(out)
}

#[tauri::command]
fn read_file_base64(app: AppHandle, path: String) -> Result<String, String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    // 仅放行 engine_dir 之下的文件(历史 PNG/截图等)。前端永远只用 list_history 返回的
    // 路径调这个命令；拼到 engine 之外的路径(/etc/passwd、AppData/Roaming/...)直接拒。
    let base = engine_dir(&app)?;
    let real = path_within(&base, Path::new(&path))?;
    let bytes = std::fs::read(&real).map_err(|e| format!("读文件失败: {e}"))?;
    Ok(STANDARD.encode(bytes))
}

/// 运行日志文件元信息(供"日志"页列出 engine/logs/ 下的日志)
#[derive(Serialize)]
struct LogFile {
    name: String,
    path: String,
    size: u64,
    modified_ms: i64,
}

/// 列出 engine/logs/ 下的所有 .log 文件，按修改时间倒序(最新在前)。
#[tauri::command]
fn list_log_files(app: AppHandle) -> Result<Vec<LogFile>, String> {
    let dir = engine_dir(&app)?.join("logs");
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("log") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let modified_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        out.push(LogFile {
            name: path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string(),
            path: path.to_string_lossy().into_owned(),
            size: meta.len(),
            modified_ms,
        });
    }
    out.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms));
    Ok(out)
}

/// 读取日志文本(UTF-8,有损解码避免个别坏字节报错)。
/// 只返回文件尾部最多 max_kb KB —— 日志关心最新内容，超大文件也不卡前端。
#[tauri::command]
fn read_log_text(app: AppHandle, path: String, max_kb: Option<u64>) -> Result<String, String> {
    // 仅 engine_dir/logs 下的 .log 文件。从其它路径打开任意文件 = 任意读，不允许。
    let base = engine_dir(&app)?.join("logs");
    let real = path_within(&base, Path::new(&path))?;
    if real.extension().and_then(|s| s.to_str()) != Some("log") {
        return Err(String::from("拒绝访问:仅支持 .log 文件"));
    }
    let cap = max_kb.unwrap_or(512) * 1024;
    let bytes = std::fs::read(&real).map_err(|e| format!("读日志失败: {e}"))?;
    let slice: &[u8] = if cap > 0 && bytes.len() as u64 > cap {
        &bytes[bytes.len() - cap as usize..]
    } else {
        &bytes[..]
    };
    Ok(String::from_utf8_lossy(slice).into_owned())
}

/// 删除一条历史记录(.log 与 .png 同时删)
#[tauri::command]
fn delete_history_item(app: AppHandle, name: String) -> Result<(), String> {
    // name 是用户输入(理论上只来自 list_history 的返回)，但仍然校验:
    // 真要拼出 results/../../../whatever 也只在 results/ 内才会被允许。
    ensure_safe_basename(&name)?;
    let dir = engine_dir(&app)?.join("results");
    for ext in ["log", "png"] {
        let p = dir.join(format!("{name}.{ext}"));
        if p.exists() {
            // 二次校验:canonicalize 后必须仍在 results/ 之内。
            let real = path_within(&dir, &p)?;
            std::fs::remove_file(&real).map_err(|e| format!("删除 {} 失败: {e}", real.display()))?;
        }
    }
    Ok(())
}

/// 清空所有历史记录
#[tauri::command]
fn clear_history(app: AppHandle) -> Result<u32, String> {
    let dir = engine_dir(&app)?.join("results");
    if !dir.exists() {
        return Ok(0);
    }
    let mut n = 0u32;
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        if ext == "log" || ext == "png" {
            if std::fs::remove_file(&path).is_ok() {
                n += 1;
            }
        }
    }
    Ok(n)
}

/// 导出一条历史:把 results/<name>.log 和 .png 复制到 target_dir。
/// dest_name 非空时用作导出文件的基名(.log/.png 同名),为空则沿用原始 name。
/// 供"另存为"对话框让用户自定义保存文件名。
#[tauri::command]
fn export_history(
    app: AppHandle,
    name: String,
    target_dir: String,
    dest_name: String,
) -> Result<Vec<String>, String> {
    // name 用于拼源路径，必须是干净的 basename。target_dir 来自系统对话框，
    // 不限定基准目录(用户主动选择的导出位置)，但仍然要求是已存在的目录。
    ensure_safe_basename(&name)?;
    let src_dir = engine_dir(&app)?.join("results");
    let dst_dir = std::path::PathBuf::from(&target_dir);
    if !dst_dir.is_dir() {
        return Err(format!("目标目录无效: {}", dst_dir.display()));
    }
    let safe_dest = dest_name.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
    let base = if safe_dest.is_empty() { name.clone() } else { safe_dest };
    let mut written = Vec::new();
    for ext in ["log", "png"] {
        let src = src_dir.join(format!("{name}.{ext}"));
        if !src.exists() {
            continue;
        }
        // 源路径越权再校一次:防御 ensure_safe_basename 之外的奇兵。
        let real_src = path_within(&src_dir, &src)?;
        let dst = dst_dir.join(format!("{base}.{ext}"));
        std::fs::copy(&real_src, &dst).map_err(|e| format!("复制失败 {}: {e}", dst.display()))?;
        written.push(dst.to_string_lossy().into_owned());
    }
    Ok(written)
}

#[derive(Serialize)]
struct GeoIpInfo {
    country: String,
    region: String,
    city: String,
    isp: String,
}

/// ip-api.com 返回的 ISP 字段是英文(China Telecom backbone 等),与 C++ 后端
/// translateIsp() 保持一致的关键词映射，前后端展示文案统一为中文。
fn translate_isp(raw: &str) -> String {
    if raw.is_empty() {
        return raw.to_string();
    }
    let lower = raw.to_lowercase();
    // 顺序敏感:长关键词先匹配
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
        ("china broadcast", "中国广电"),
        ("chinabroadnet", "中国广电"),
        ("cbn", "中国广电"),
        ("great wall broadband", "长城宽带"),
        ("great wall", "长城宽带"),
        ("dr.peng", "鹏博士"),
        ("dr peng", "鹏博士"),
        ("cernet", "教育网"),
        ("education network", "教育网"),
        ("cstnet", "中国科技网"),
        ("cnix", "中国国际通信网"),
        ("cloudflare", "Cloudflare"),
        ("amazon", "Amazon AWS"),
        ("google", "Google"),
        ("microsoft", "Microsoft Azure"),
        ("akamai", "Akamai"),
        ("hurricane electric", "Hurricane Electric"),
    ];
    for (kw, zh) in rules {
        if lower.contains(kw) {
            return zh.to_string();
        }
    }
    raw.to_string()
}

/// 单次拉取 ip-api.com 中文版，返回结构化 GeoIpInfo。失败原因以 String 抛回。
async fn fetch_my_ip_info_once(client: &reqwest::Client) -> Result<GeoIpInfo, String> {
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
        return Err(format!("ip-api 返回错误: {msg}"));
    }
    let pick = |k: &str| -> String {
        json.get(k)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    Ok(GeoIpInfo {
        country: pick("country"),
        region: pick("regionName"),
        city: pick("city"),
        isp: translate_isp(&pick("isp")),
    })
}

/// 查询本机出口公网 IP 与地理位置/运营商。用 ip-api.com 中文接口，失败时
/// 自动重试最多 5 次，每次间隔 3 秒(ip-api 免费版偶发限频或瞬时网络抖动
/// 会让单次请求失败，重试可显著提升 footer 出数据的概率)。
///
/// 必须 .no_proxy():这个查询的语义是"本机真实出口在哪"。启动时若读到
/// Windows 系统代理(mihomo)会注入 HTTPS_PROXY/HTTP_PROXY,reqwest 默认
/// 读 env 自动走代理 → ip-api.com 看到的源 IP 是代理节点出口，用户位置
/// 显示成代理节点归属地。直连才能拿到本机真实出口。
#[tauri::command]
async fn get_my_ip_info() -> Result<GeoIpInfo, String> {
    use std::net::{IpAddr, Ipv4Addr};
    const MAX_ATTEMPTS: u32 = 5;
    let client = reqwest::Client::builder()
        .no_proxy()
        // 强制本地 IPv4 套接字出网，避免 IPv6 接口被解析到 IPv6 节点
        .local_address(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
        .timeout(std::time::Duration::from_secs(8))
        .user_agent("Mozilla/5.0 StairSpeedtest")
        .build()
        .map_err(|e| e.to_string())?;

    let mut last_err = String::from("未发起请求");
    for attempt in 1..=MAX_ATTEMPTS {
        match fetch_my_ip_info_once(&client).await {
            Ok(info) => return Ok(info),
            Err(e) => {
                last_err = e;
                eprintln!(
                    "get_my_ip_info attempt {attempt}/{MAX_ATTEMPTS} failed: {last_err}"
                );
                if attempt < MAX_ATTEMPTS {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                }
            }
        }
    }
    Err(format!("已重试 {MAX_ATTEMPTS} 次仍失败: {last_err}"))
}

/// 编译时记录的构建时间戳(用于运行时识别"装的是哪版本")
const BUILD_TIME: &str = env!("BUILD_TIME");
const NOPROXY_TAG: &str = "noproxy-v3";

/// 启动时把版本/构建时间/proxy 模式写到 app-startup.log,
/// 任何"还是没变化"类型的反馈都能直接读这个日志确认
fn write_startup_banner(app: &AppHandle) {
    use std::io::Write;
    let Ok(dir) = engine_dir(app) else { return };
    let logs = dir.join("logs");
    let _ = std::fs::create_dir_all(&logs);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs.join("app-startup.log"))
    {
        let now = chrono_now();
        let _ = writeln!(
            f,
            "[{}] startup pkg={} build={} proxy_mode={} runtime_engine={}",
            now,
            env!("CARGO_PKG_VERSION"),
            BUILD_TIME,
            NOPROXY_TAG,
            dir.display()
        );
    }
}

/// 仅用于日志的 yyyy-mm-dd HH:MM:SS 时间字符串(避免引入 chrono 依赖)
fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64
        + 8 * 3600; // 北京时间
    let days = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    // 简陋年月日(够日志辨认时间即可)
    let (y, mo, d) = days_to_ymd(days);
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, mo, d, h, m, s)
}

fn days_to_ymd(mut days: i64) -> (i64, u32, u32) {
    days += 719468;
    let era = if days >= 0 { days } else { days - 146096 } / 146097;
    let doe = (days - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// 后台跑一次 .no_proxy() 自检:
/// 等后端起来后向 /getversion 发 GET,把结果写到 app-startup.log。
/// 如果用户机器上系统代理拦截了 127.0.0.1,这条记录会显示成功(因为 .no_proxy 生效),
/// 否则会有错误信息可以直接定位。
async fn self_check(app: AppHandle) {
    use std::io::Write;
    // 等后端起来 + 监听端口稳定
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    let result = match local_backend_client()
        .get(format!("{}/getversion", BACKEND_URL))
        .send()
        .await
    {
        Ok(r) => format!("OK status={} body={}", r.status(), r.text().await.unwrap_or_default()),
        Err(e) => format!("ERR {e}"),
    };
    let proxy_env = std::env::var("HTTP_PROXY")
        .or_else(|_| std::env::var("http_proxy"))
        .unwrap_or_else(|_| "(none)".into());
    let Ok(dir) = engine_dir(&app) else { return };
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("logs").join("app-startup.log"))
    {
        let _ = writeln!(
            f,
            "[{}] selfcheck HTTP_PROXY={} -> {}",
            chrono_now(),
            proxy_env,
            result
        );
    }
}

/// 下载最新 mihomo 内核覆盖 runtime engine 下的二进制(只动 user 可写目录，不动 bundle)。
/// 资产按当前 OS+arch 选 zip(Windows)或 gz(Linux/macOS),失败时用 .bak 还原。
#[derive(Serialize)]
struct UpdateResult {
    success: bool,
    new_version: String,
    error: String,
}

// 应用自身更新现在由 tauri-plugin-updater 处理:
// 客户端读 GitHub release 里的 latest.json,minisign 公钥验签,
// 三平台自动 download + install + relaunch。无需后端命令对应。

#[tauri::command]
async fn download_mihomo_update(app: AppHandle) -> Result<UpdateResult, String> {
    use std::io::Read;

    // 不能 .no_proxy():国内直连 GitHub 资产域必走系统代理才能下载成功
    let dl_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .user_agent("stairspeedtest-reborn-desktop")
        .build()
        .map_err(|e| format!("创建下载 client 失败: {e}"))?;

    let resp = dl_client
        .get("https://api.github.com/repos/MetaCubeX/mihomo/releases/latest")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("查询最新版本失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("GitHub API 返回 {}", resp.status()));
    }
    let release: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析 release JSON 失败: {e}"))?;

    let tag = release
        .get("tag_name")
        .and_then(|v| v.as_str())
        .ok_or("release 缺少 tag_name 字段")?
        .to_string();

    let assets = release
        .get("assets")
        .and_then(|v| v.as_array())
        .ok_or("release 缺少 assets")?;

    // 与 desktop.yml CI 装包时的资产命名保持一致，避免初装版与更新后版本不同源
    let (asset_prefix, asset_ext): (&str, &str) =
        if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            ("mihomo-windows-amd64-", ".zip")
        } else if cfg!(all(target_os = "windows", target_arch = "aarch64")) {
            ("mihomo-windows-arm64-", ".zip")
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            ("mihomo-linux-amd64-", ".gz")
        } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
            ("mihomo-linux-arm64-", ".gz")
        } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
            ("mihomo-darwin-amd64-", ".gz")
        } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            ("mihomo-darwin-arm64-", ".gz")
        } else {
            return Err("当前平台/架构不支持自动更新 mihomo".into());
        };
    let target_name = format!("{asset_prefix}{tag}{asset_ext}");
    let asset = assets
        .iter()
        .find(|a| a.get("name").and_then(|n| n.as_str()) == Some(target_name.as_str()))
        .ok_or_else(|| format!("release 资产中未找到 {target_name}"))?;
    let download_url = asset
        .get("browser_download_url")
        .and_then(|v| v.as_str())
        .ok_or("asset 缺少 browser_download_url")?;

    let dl_resp = dl_client
        .get(download_url)
        .send()
        .await
        .map_err(|e| format!("下载 {target_name} 失败: {e}"))?;
    if !dl_resp.status().is_success() {
        let code = dl_resp.status();
        let hint = match code.as_u16() {
            504 => "(GitHub 资产域被中间网关超时，请确认系统代理已开启)",
            403 => "(GitHub 速率限制或资产临时不可用，稍后重试)",
            _ => "",
        };
        return Err(format!("下载返回 {code} {hint}"));
    }
    let raw_bytes = dl_resp
        .bytes()
        .await
        .map_err(|e| format!("读取响应数据失败: {e}"))?;

    // zip 内有一个 mihomo* 二进制条目;gz 是裸二进制流
    let bin_bytes: Vec<u8> = if asset_ext == ".zip" {
        let cursor = std::io::Cursor::new(raw_bytes.as_ref());
        let mut archive = zip::ZipArchive::new(cursor)
            .map_err(|e| format!("zip 格式错误: {e}"))?;
        let mut buf: Option<Vec<u8>> = None;
        for i in 0..archive.len() {
            let mut entry = archive
                .by_index(i)
                .map_err(|e| format!("读取 zip 条目 {i} 失败: {e}"))?;
            if entry.is_file() && entry.name().to_ascii_lowercase().contains("mihomo") {
                let mut data = Vec::with_capacity(entry.size() as usize);
                entry
                    .read_to_end(&mut data)
                    .map_err(|e| format!("解压条目失败: {e}"))?;
                buf = Some(data);
                break;
            }
        }
        buf.ok_or("zip 中未找到 mihomo 二进制")?
    } else {
        let mut decoder = flate2::read::GzDecoder::new(raw_bytes.as_ref());
        let mut data = Vec::new();
        decoder
            .read_to_end(&mut data)
            .map_err(|e| format!("gz 解压失败: {e}"))?;
        data
    };

    let dir = runtime_engine_dir(&app)?;
    let clients_dir = dir.join("tools").join("clients");
    if !clients_dir.exists() {
        return Err(format!("目标目录不存在: {}", clients_dir.display()));
    }
    let mihomo_path = clients_dir.join(MIHOMO_EXE);
    let backup_path = clients_dir.join(format!("{MIHOMO_EXE}.bak"));

    // 文件被占用就无法覆盖写，先停掉在跑的 mihomo
    #[cfg(windows)]
    {
        let _ = silent_command("taskkill")
            .args(["/F", "/IM", MIHOMO_EXE, "/T"])
            .status();
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
    std::thread::sleep(std::time::Duration::from_millis(400));

    if mihomo_path.exists() {
        std::fs::copy(&mihomo_path, &backup_path)
            .map_err(|e| format!("备份原 {MIHOMO_EXE} 失败: {e}"))?;
    }

    if let Err(e) = std::fs::write(&mihomo_path, &bin_bytes) {
        if backup_path.exists() {
            let _ = std::fs::copy(&backup_path, &mihomo_path);
        }
        return Err(format!("写入新 {MIHOMO_EXE} 失败: {e}"));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(
            &mihomo_path,
            std::fs::Permissions::from_mode(0o755),
        );
    }
    // macOS 首次执行会被 Gatekeeper 拦，清掉下载来源标记
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("xattr")
            .args(["-d", "com.apple.quarantine"])
            .arg(&mihomo_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    Ok(UpdateResult {
        success: true,
        new_version: tag,
        error: String::new(),
    })
}

/// Windows 启动时把系统代理(注册表 ProxyEnable + ProxyServer)注入到进程环境变量。
/// reqwest(tauri-plugin-updater 用的)只读 HTTP_PROXY/HTTPS_PROXY,不读 Windows 注册表。
/// 用户开了系统代理(mihomo/Clash 默认行为)→ 注入后 plugin-updater 自动走代理 → 下载快;
/// 没开 → 这里 noop,plugin-updater 直连(原有行为)。用 reg query 命令避免引入 winreg 依赖。
#[cfg(target_os = "windows")]
fn apply_windows_system_proxy_env() {
    use std::process::Command;
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // 用户已经手动设置过环境变量时不覆盖(尊重显式配置)
    let user_already_set = std::env::var("HTTP_PROXY").is_ok()
        || std::env::var("HTTPS_PROXY").is_ok()
        || std::env::var("http_proxy").is_ok()
        || std::env::var("https_proxy").is_ok();
    if user_already_set {
        return;
    }

    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings";
    let read = |value: &str| -> Option<String> {
        let out = Command::new("reg")
            .args(["query", key, "/v", value])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        // reg query 输出格式: "    <name>    REG_DWORD    0x1" 或 "    <name>    REG_SZ    127.0.0.1:7897"。
        // 用 split_whitespace 兼容任意连续空格/制表符，避免 splitn 在连续空格下被切空段。
        for line in text.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 && parts[0] == value {
                return Some(parts[2..].join(" "));
            }
        }
        None
    };

    let enable = read("ProxyEnable").unwrap_or_default();
    let enabled = enable == "0x1" || enable.ends_with("0x1");
    if !enabled {
        return;
    }
    let server = match read("ProxyServer") {
        Some(s) if !s.is_empty() => s,
        _ => return,
    };

    // ProxyServer 形态: "127.0.0.1:7897" 或 "http=127.0.0.1:7897;https=127.0.0.1:7897;..."
    // 取第一个有效 host:port,前缀加 http:// 让 reqwest 解析为代理 URL
    let host_port = if server.contains('=') {
        server
            .split(';')
            .filter_map(|seg| seg.split_once('='))
            .find_map(|(scheme, addr)| {
                let s = scheme.trim().to_ascii_lowercase();
                if s == "http" || s == "https" || s == "all" {
                    Some(addr.trim().to_string())
                } else {
                    None
                }
            })
    } else {
        Some(server.trim().to_string())
    };
    let Some(host_port) = host_port.filter(|s| !s.is_empty()) else {
        return;
    };
    let proxy_url = if host_port.starts_with("http://") || host_port.starts_with("https://") {
        host_port
    } else {
        format!("http://{host_port}")
    };

    // ProxyOverride 是 ; 分隔,reqwest NO_PROXY 用 , 分隔。本地回环必加,
    // 否则 plugin 自检 / 后端 /api 都会被代理转发引发 connect refused。
    let overrides = read("ProxyOverride").unwrap_or_default();
    let mut no_proxy_list: Vec<String> = overrides
        .split(';')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "<local>")
        .collect();
    for must in ["127.0.0.1", "localhost", "::1"] {
        if !no_proxy_list.iter().any(|x| x == must) {
            no_proxy_list.push(must.to_string());
        }
    }
    let no_proxy = no_proxy_list.join(",");

    eprintln!("[proxy] using system proxy {proxy_url} (NO_PROXY={no_proxy})");
    std::env::set_var("HTTP_PROXY", &proxy_url);
    std::env::set_var("HTTPS_PROXY", &proxy_url);
    std::env::set_var("NO_PROXY", &no_proxy);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Linux 上 webkit2gtk 4.1 (2.46+) 在 Ubuntu 25.04 / Fedora 40+ / Wayland / 部分
    // NVIDIA / 老 Intel iGPU 等环境组合下，默认开启的 DMABUF 渲染器 + GPU 合成存在
    // 大量已知 bug,典型表现就是窗口黑屏 / 白屏 / 完全不显示(用户视角:"双击图标
    // 没反应")。社区有两条非常成熟的 workaround,都是关掉对应渲染路径回退到软件
    // 渲染，代价仅是少量性能损失，稳定性显著提升。相关 tauri-apps/tauri issue:
    //   #5143  Ubuntu blank screen → WEBKIT_DISABLE_COMPOSITING_MODE=1
    //   #13183 Manjaro NVIDIA 黑屏 → WEBKIT_DISABLE_DMABUF_RENDERER=1
    //   #15050 Fedora 43 + Sway 黑屏(Wayland)
    //   #13414 Ubuntu 25.04 GTK backend 初始化失败
    //
    // 这两条变量必须在创建 webview 前设好(Tauri::Builder 一旦初始化 webkit 就晚了),
    // 所以放在 run() 最前面。仅 Linux 设置;其他平台不受影响。
    // 用户已在环境里手动设置过任一变量时,std::env::set_var 仍幂等覆盖为 "1",
    // 这与社区文档的推荐值一致，不会破坏用户的自定义。
    #[cfg(target_os = "linux")]
    {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
    }

    // tauri-plugin-updater 内部用 reqwest 拉 latest.json + setup.exe。
    // reqwest 默认只读 HTTP_PROXY / HTTPS_PROXY 环境变量，不会读 Windows 系统代理
    // (HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings)。
    // 国内大陆直连 GitHub releases 经常超时或几十 KB/s,所以这里在启动时主动读注册表,
    // 把 mihomo / Clash 之类设置的系统代理 URL 注入到进程环境变量，让 reqwest 自动走代理。
    // NO_PROXY 兜住 127.0.0.1 / localhost,避免 plugin 自检和后端 API 请求被代理拦截。
    #[cfg(target_os = "windows")]
    apply_windows_system_proxy_env();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(BackendState {
            child: Mutex::new(None),
        })
        .setup(|app| {
            // 启动前清理可能残留的孤儿进程(尤其是 mihomo 内核),
            // 防止端口/句柄被占用导致新后端测试中途异常退出。
            cleanup_orphans();
            let handle = app.handle().clone();
            // 关键:先把打包的 engine 同步到用户可写位置，避免 Program Files 写权限问题
            if let Err(e) = ensure_runtime_engine(&handle) {
                eprintln!("[engine] 同步运行时目录失败: {e}");
            }
            // 写启动横幅到 engine/logs/app-startup.log,方便排查"装的是哪版"
            write_startup_banner(&handle);
            // 后台跑 .no_proxy() 自检，把结果写到 app-startup.log
            tauri::async_runtime::spawn(self_check(handle.clone()));
            // 修复任务栏图标模糊:Tauri 2.11.2 仅注入 ICO 第一个条目(我们这是 16x16),
            // 这里在主窗口创建后立刻用 Win32 API 重设 ICON_BIG/ICON_SMALL 为高分辨率版本。
            // 详见 apply_high_res_icons 上方的注释。
            if let Some(w) = handle.get_webview_window("main") {
                apply_high_res_icons(&w);
            }
            match spawn_backend(&handle) {
                Ok(c) => {
                    let state = handle.state::<BackendState>();
                    // 中毒(panic 时持锁线程崩了)的话还能拿到 PoisonError，
                    // 这里取 inner 继续用:Option<Child> 不依赖临时不一致状态。
                    let mut guard = state.child.lock().unwrap_or_else(|e| e.into_inner());
                    *guard = Some(c);
                }
                Err(e) => eprintln!("[backend] 启动失败: {e}"),
            }
            // 启动 supervisor:崩了自动重启,UI 不会一直停在"后端未连接"
            tauri::async_runtime::spawn(supervisor_loop(handle.clone()));
            // 兜底:即使前端因 webkit/JS 异常没能调 show_main_window,
            // 5 秒后主进程也强制让主窗口可见，避免"双击没反应"的体感。
            let h_show = handle.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                if let Some(w) = h_show.get_webview_window("main") {
                    if !w.is_visible().unwrap_or(true) {
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            backend_url,
            show_main_window,
            api_get,
            api_post_json,
            api_post_file,
            import_config_file,
            restart_backend,
            clear_app_data,
            list_history,
            read_file_base64,
            list_log_files,
            read_log_text,
            delete_history_item,
            clear_history,
            export_history,
            get_my_ip_info,
            download_mihomo_update
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|handle, event| {
            if let RunEvent::Exit = event {
                // 关键:必须先置位让 supervisor 退出循环，否则它可能在主进程
                // kill 子进程的瞬间检测到"已退出"再次拉起 mihomo,留下孤儿。
                SHUTTING_DOWN.store(true, Ordering::SeqCst);
                let state = handle.state::<BackendState>();
                // 退出路径上若锁中毒就取 inner 继续走:即便 panic 留了垃圾态,
                // 我们的目标只是 take() 出 child 把它收掉，语义不依赖锁的健康。
                let mut guard = state.child.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(mut c) = guard.take() {
                    kill_backend_tree(&mut c);
                }
            }
        });
}
