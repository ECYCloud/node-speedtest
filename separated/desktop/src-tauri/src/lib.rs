mod engine;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Manager, RunEvent, State};

use engine::Engine;

/// 退出标志:clear_app_data / RunEvent::Exit 时置位。
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

#[cfg(windows)]
const MIHOMO_EXE: &str = "mihomo.exe";
#[cfg(not(windows))]
const MIHOMO_EXE: &str = "mihomo";

/// 进程内测速引擎状态。
struct BackendState {
    engine: Arc<Engine>,
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
/// 指纹基于 CARGO_PKG_VERSION + mihomo 二进制 size/mtime。
///
/// 复制策略:字体等出图关键资产优先、单文件失败不中断整树，避免
/// `tools/clients/mihomo.exe` 被占用时整次同步中止，导致永远没有字体、结果图失败。
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
        let src_mihomo = src.join("tools").join("clients").join(MIHOMO_EXE);

        let sentinels: Vec<PathBuf> = vec![
            PathBuf::from("tools/clients").join(MIHOMO_EXE),
            PathBuf::from("tools/misc").join("SourceHanSansCN-Medium.otf"),
            PathBuf::from("pref.ini"),
        ];
        // 出图强依赖字体；mihomo 占用时也必须先落到盘上
        let priority: Vec<PathBuf> = vec![
            PathBuf::from("tools/misc").join("SourceHanSansCN-Medium.otf"),
            PathBuf::from("tools/misc").join("TwemojiFlat.ttf"),
            PathBuf::from("pref.ini"),
            PathBuf::from("tools/clients").join(MIHOMO_EXE),
        ];

        let want_sig = compute_engine_signature(&src_mihomo);
        let sig_path = dst.join(".runtime_fingerprint");
        let have_sig = std::fs::read_to_string(&sig_path).ok();

        let runtime_complete = sentinels.iter().all(|rel| dst.join(rel).exists());
        if runtime_complete && have_sig.as_deref() == Some(want_sig.as_str()) {
            return Ok(());
        }

        std::fs::create_dir_all(&dst).map_err(|e| format!("创建运行时目录失败: {e}"))?;
        let _ = std::fs::create_dir_all(dst.join("logs"));
        let _ = std::fs::create_dir_all(dst.join("results"));
        engine::rlog::info(
            &dst,
            format!("同步 runtime engine ← {}", src.display()),
        );

        for rel in &priority {
            let from = src.join(rel);
            let to = dst.join(rel);
            if !from.is_file() {
                engine::rlog::error(&dst, format!("安装包缺少: {}", from.display()));
                continue;
            }
            if let Err(e) = copy_file_retry(&from, &to) {
                engine::rlog::warn(&dst, format!("优先同步失败 {}: {e}", rel.display()));
            }
        }

        let skipped = copy_dir_excluding_best_effort(&src, &dst, &["logs", "results"]);
        for e in skipped {
            engine::rlog::warn(&dst, format!("同步跳过: {e}"));
        }

        let missing: Vec<String> = sentinels
            .iter()
            .filter(|rel| !dst.join(rel).exists())
            .map(|rel| rel.display().to_string())
            .collect();
        if !missing.is_empty() {
            let msg = format!("runtime 资产不完整: {}", missing.join(", "));
            engine::rlog::error(&dst, &msg);
            return Err(msg);
        }
        let _ = std::fs::write(&sig_path, &want_sig);
        engine::rlog::info(&dst, "runtime engine 同步完成");
        Ok(())
    }
}

/// 计算 bundled mihomo 的版本指纹，用作 runtime engine 是否需要重同步的判定。
#[cfg(not(debug_assertions))]
fn compute_engine_signature(src_bin: &std::path::Path) -> String {
    let (size, mtime) = match std::fs::metadata(src_bin) {
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
        "v{} mihomo-size={} mtime={}",
        env!("CARGO_PKG_VERSION"),
        size,
        mtime
    )
}

/// 覆盖复制单文件；Windows 上文件刚被 taskkill 时可能短暂占用，重试几次。
#[cfg_attr(debug_assertions, allow(dead_code))]
fn copy_file_retry(src: &Path, dst: &Path) -> Result<(), String> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    let mut last = String::new();
    for attempt in 1..=5u32 {
        match std::fs::copy(src, dst) {
            Ok(_) => return Ok(()),
            Err(e) => {
                last = format!("copy {} -> {}: {e}", src.display(), dst.display());
                std::thread::sleep(std::time::Duration::from_millis(80 * u64::from(attempt)));
            }
        }
    }
    Err(last)
}

/// 递归复制目录，跳过指定子目录(按基名)。单文件失败记入返回列表，不中断其余文件。
#[cfg_attr(debug_assertions, allow(dead_code))]
fn copy_dir_excluding_best_effort(src: &Path, dst: &Path, exclude: &[&str]) -> Vec<String> {
    let mut errors = Vec::new();
    if let Err(e) = std::fs::create_dir_all(dst) {
        errors.push(format!("mkdir {}: {e}", dst.display()));
        return errors;
    }
    let entries = match std::fs::read_dir(src) {
        Ok(v) => v,
        Err(e) => {
            errors.push(format!("read_dir {}: {e}", src.display()));
            return errors;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(v) => v,
            Err(e) => {
                errors.push(e.to_string());
                continue;
            }
        };
        let name_os = entry.file_name();
        let name = name_os.to_string_lossy();
        if exclude.iter().any(|x| *x == name.as_ref()) {
            continue;
        }
        let s = entry.path();
        let d = dst.join(&*name);
        let ft = match entry.file_type() {
            Ok(v) => v,
            Err(e) => {
                errors.push(format!("{}: {e}", s.display()));
                continue;
            }
        };
        if ft.is_dir() {
            errors.extend(copy_dir_excluding_best_effort(&s, &d, exclude));
        } else if let Err(e) = copy_file_retry(&s, &d) {
            errors.push(e);
        }
    }
    errors
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

/// 启动前清理可能残留的孤儿 mihomo，避免端口冲突。
#[cfg(windows)]
fn cleanup_orphans() {
    let _ = silent_command("taskkill")
        .args(["/F", "/IM", "mihomo.exe", "/T"])
        .status();
}

/// Linux/macOS 走 pkill -f,匹配命令路径(sidecar 用绝对路径起 mihomo)。
#[cfg(not(windows))]
fn cleanup_orphans() {
    for name in ["mihomo"] {
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

#[tauri::command]
fn backend_url() -> &'static str {
    "in-process"
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


/// 轻量 HTTP 客户端:供外网请求等非引擎路径复用。
#[allow(dead_code)]
fn local_http_client() -> &'static reqwest::Client {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(std::time::Duration::from_secs(2))
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .expect("无法创建 local http client")
    })
}

#[tauri::command]
async fn api_get(path: String, state: State<'_, BackendState>) -> Result<String, String> {
    state.engine.handle_get(&path).await
}

#[tauri::command]
async fn api_post_json(
    path: String,
    body: String,
    state: State<'_, BackendState>,
) -> Result<String, String> {
    state.engine.handle_post_json(&path, &body).await
}

#[tauri::command]
async fn api_post_file(
    path: String,
    file_name: String,
    file_bytes: Vec<u8>,
    state: State<'_, BackendState>,
) -> Result<String, String> {
    let _ = file_name;
    state.engine.handle_post_file(&path, file_bytes).await
}

#[tauri::command]
async fn import_config_file(
    path: String,
    state: State<'_, BackendState>,
) -> Result<String, String> {
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
    state.engine.handle_post_file("/readfileconfig", bytes).await
}

#[tauri::command]
async fn restart_backend(app: AppHandle, state: State<'_, BackendState>) -> Result<(), String> {
    ensure_runtime_engine(&app)?;
    if let Ok(dir) = runtime_engine_dir(&app) {
        engine::rlog::info(&dir, "设置：用户请求重启后端");
    }
    cleanup_orphans();
    match state.engine.restart().await {
        Ok(()) => {
            if let Ok(dir) = runtime_engine_dir(&app) {
                engine::rlog::info(&dir, "设置：后端重启完成");
            }
            Ok(())
        }
        Err(e) => {
            if let Ok(dir) = runtime_engine_dir(&app) {
                engine::rlog::error(&dir, format!("设置：后端重启失败: {e}"));
            }
            Err(e)
        }
    }
}

/// 供前端把设置页等用户操作写入 runtime.log。
#[tauri::command]
fn append_runtime_log(
    app: AppHandle,
    level: String,
    message: String,
) -> Result<(), String> {
    let dir = runtime_engine_dir(&app)?;
    match level.to_ascii_uppercase().as_str() {
        "WARN" | "WARNING" => engine::rlog::warn(&dir, message),
        "ERROR" => engine::rlog::error(&dir, message),
        _ => engine::rlog::info(&dir, message),
    }
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
async fn clear_app_data(
    app: AppHandle,
    state: State<'_, BackendState>,
) -> Result<ClearAppDataResult, String> {
    #[cfg(debug_assertions)]
    {
        if let Ok(dir) = runtime_engine_dir(&app) {
            engine::rlog::warn(&dir, "设置：清理应用数据被拒绝（开发模式）");
        }
        let _ = (app, state);
        return Err("开发模式下禁止清理应用数据，以免误删仓库 engine 目录".into());
    }
    #[cfg(not(debug_assertions))]
    {
        if let Ok(dir) = runtime_engine_dir(&app) {
            engine::rlog::warn(&dir, "设置：用户确认清理应用数据（将删除 engine 并重启）");
        }
        SHUTTING_DOWN.store(true, Ordering::SeqCst);
        // 等批次写盘结束再删目录，避免 results/ 残留或删写竞态
        state
            .engine
            .shutdown_and_wait(std::time::Duration::from_secs(45))
            .await?;
        cleanup_orphans();

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

        // 4. panic.log 在 %APPDATA%\com.nodespeedtest.desktop 下，与 main.rs 的
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
    let engine = engine_dir(&app)?;
    let dir = engine.join("results");
    // 目录不存在视为空列表（新装常见），不写运行日志以免进页刷屏
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut grouped: BTreeMap<String, HistoryItem> = BTreeMap::new();
    let entries = std::fs::read_dir(&dir).map_err(|e| {
        let msg = format!("读取历史目录失败 {}: {e}", dir.display());
        engine::rlog::error(&engine, &msg);
        msg
    })?;
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()).map(String::from) else {
            continue;
        };
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ext != "log" && ext != "png" {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
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
        if ext == "log" {
            item.log_path = Some(p);
        } else {
            item.image_path = Some(p);
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
    // 运行日志优先，其余按修改时间倒序
    out.sort_by(|a, b| {
        let rank = |n: &str| match n {
            "runtime.log" => 0,
            "app-startup.log" => 1,
            _ => 2,
        };
        rank(&a.name)
            .cmp(&rank(&b.name))
            .then_with(|| b.modified_ms.cmp(&a.modified_ms))
    });
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
        .user_agent("Mozilla/5.0 NodeSpeedtest")
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
    let now = chrono_now();
    let banner = format!(
        "startup pkg={} build={} proxy_mode={} runtime_engine={}",
        env!("CARGO_PKG_VERSION"),
        BUILD_TIME,
        NOPROXY_TAG,
        dir.display()
    );
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs.join("app-startup.log"))
    {
        let _ = writeln!(f, "[{now}] {banner}");
    }
    engine::rlog::info(&dir, banner);
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
async fn download_mihomo_update(
    app: AppHandle,
    state: State<'_, BackendState>,
) -> Result<UpdateResult, String> {
    use std::io::Read;

    if let Ok(dir) = runtime_engine_dir(&app) {
        engine::rlog::info(&dir, "设置：开始下载并安装 mihomo 内核更新");
    }

    if state.engine.is_running() {
        if let Ok(dir) = runtime_engine_dir(&app) {
            engine::rlog::warn(&dir, "设置：mihomo 更新被拒绝（测速进行中）");
        }
        return Err("测速进行中，请先停止后再更新内核".into());
    }

    // 不能 .no_proxy():国内直连 GitHub 资产域必走系统代理才能下载成功
    let dl_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .user_agent("NodeSpeedtest-desktop")
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

    // 下载期间用户可能又点了测速：替换前再拦一次，并等批次真正收尾
    if state.engine.is_running() {
        if let Ok(dir) = runtime_engine_dir(&app) {
            engine::rlog::warn(&dir, "设置：mihomo 更新被拒绝（测速进行中）");
        }
        return Err("测速进行中，请先停止后再更新内核".into());
    }
    if !state
        .engine
        .await_idle(std::time::Duration::from_secs(45))
        .await
    {
        if let Ok(dir) = runtime_engine_dir(&app) {
            engine::rlog::warn(&dir, "设置：mihomo 更新被拒绝（测速任务未停净）");
        }
        return Err("测速任务未能停止，请先停止测速后再更新内核".into());
    }

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

    // 安装结果以磁盘 `mihomo -v` 为准，不用 GitHub tag 口头宣称
    let verified = engine::read_version_from_binary(&dir);
    let new_version = if verified.is_empty() { tag } else { verified };
    engine::rlog::info(
        &dir,
        format!("设置：mihomo 内核已更新到 {new_version}"),
    );
    Ok(UpdateResult {
        success: true,
        new_version,
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
        .setup(|app| {
            cleanup_orphans();
            let handle = app.handle().clone();
            if let Err(e) = ensure_runtime_engine(&handle) {
                eprintln!("[engine] 同步运行时目录失败: {e}");
            }
            let work = engine_dir(&handle).unwrap_or_else(|_| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("engine")
            });
            let eng = Arc::new(Engine::new(work));
            app.manage(BackendState {
                engine: Arc::clone(&eng),
            });
            write_startup_banner(&handle);
            let st = handle.state::<BackendState>();
            let eng2 = Arc::clone(&st.engine);
            let handle_log = handle.clone();
            tauri::async_runtime::spawn(async move {
                use std::io::Write;
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                let result = match eng2.handle_get("/getversion").await {
                    Ok(body) => format!("OK in-process body={body}"),
                    Err(e) => format!("ERR {e}"),
                };
                let proxy_env = std::env::var("HTTP_PROXY")
                    .or_else(|_| std::env::var("http_proxy"))
                    .unwrap_or_else(|_| "(none)".into());
                let Ok(dir) = engine_dir(&handle_log) else { return };
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
                engine::rlog::info(
                    &dir,
                    format!("selfcheck HTTP_PROXY={proxy_env} -> {result}"),
                );
            });
            if let Some(w) = handle.get_webview_window("main") {
                apply_high_res_icons(&w);
            }
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
            append_runtime_log,
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
                SHUTTING_DOWN.store(true, Ordering::SeqCst);
                if let Some(state) = handle.try_state::<BackendState>() {
                    state.engine.shutdown();
                }
                cleanup_orphans();
            }
        });
}
