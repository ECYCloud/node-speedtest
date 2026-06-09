use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Manager, RunEvent, State};

/// 后端进程统一监听地址,与 stairspeedtest pref.ini 的 [webserver] 默认值保持一致
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

/// 持有后端子进程句柄,应用退出时统一收尾
struct BackendState {
    child: Mutex<Option<Child>>,
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
/// 如果直接在打包目录里跑会被 UAC 静默拒绝,导致测试结果全 N/A 但不报错。
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
///      失败,只复制了根目录的 DLL/exe,tools/ 子树整个丢了,但 exe 指纹依然
///      "看起来匹配",于是后续所有启动都 skip,导致 mihomo 永远拉不起来 +
///      字体加载失败 + 测试结果全 0/N/A。
///      现在改为只要任意一个 sentinel 缺失就强制重新同步。
///
/// 用户的 logs/ 与 results/(历史记录、测试日志)始终保留,只覆盖 exe / DLL /
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
        let dst_exe = dst.join(ENGINE_EXE);

        // 取 (size, mtime) 作为指纹,任一不同就重新同步
        let fingerprint = |p: &std::path::Path| -> Option<(u64, std::time::SystemTime)> {
            let m = std::fs::metadata(p).ok()?;
            Some((m.len(), m.modified().ok()?))
        };

        // 关键资产 sentinel:这些文件缺一个就视为 runtime 不完整,必须重同步
        let sentinels = [
            std::path::PathBuf::from("tools/clients").join(MIHOMO_EXE),
            std::path::PathBuf::from("tools/misc/SourceHanSansCN-Medium.otf"),
            std::path::PathBuf::from("config.yaml"),
            std::path::PathBuf::from("pref.ini"),
        ];
        let runtime_complete = sentinels.iter().all(|rel| dst.join(rel).exists());

        if runtime_complete {
            if let (Some(a), Some(b)) = (fingerprint(&src_exe), fingerprint(&dst_exe)) {
                if a == b {
                    return Ok(()); // 完整且 exe 指纹一致,无需同步
                }
            }
        }

        std::fs::create_dir_all(&dst).map_err(|e| format!("创建运行时目录失败: {e}"))?;
        // 只同步只读资产,跳过用户数据(logs / results / cache.db)
        copy_dir_excluding(&src, &dst, &["logs", "results"])?;
        let _ = std::fs::create_dir_all(dst.join("logs"));
        let _ = std::fs::create_dir_all(dst.join("results"));
        // 旧 cache.db 不会清,mihomo 会自己处理过期条目
        Ok(())
    }
}

/// 递归复制目录,跳过指定子目录(按基名)
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

    // stderr/stdout 写入日志文件,便于排查 mihomo 启动失败等问题。
    // 这比 Stdio::null() 更友好——出问题能直接看日志,而不是黑盒。
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

/// 在 Windows 上调用 taskkill / 其他外部命令,加 CREATE_NO_WINDOW 防止闪现 cmd 窗口
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
// 现象:应用启动几秒后,任务栏图标突然变模糊。
//
// 根因:Tauri 2.11.2 的 tauri-codegen 在编译期解析 bundle.icon 时,只读取 ICO 文件的
// 第一个条目(`entries()[0]`)作为运行时窗口图标 RGBA 缓冲(官方 issue #14596 /
// PR #15241,2.11.2 未修)。我们的 icon.ico 按惯例从小到大排列,第一个条目是 16x16,
// 被 wry 通过 WM_SETICON 设给主窗口后,任务栏(需要 32/48 px)就只能从 16x16 上采样,
// Alt-Tab、任务栏预览全部被替换成糊图。EXE 嵌入资源里的多分辨率 ICO 在窗口创建前
// 一直被系统使用,所以"刚启动还清晰、过几秒变糊"。
//
// 修复:绕过 Tauri 内部那张 16x16 RGBA,在 setup 拿到主窗口 HWND 后,自己用 Win32
// API 从完整的 ICO 字节里挑出最匹配 ICON_BIG/ICON_SMALL 期望尺寸的条目,
// CreateIconFromResourceEx 创建 HICON,再 WM_SETICON 覆盖。这样:
//   - ICON_BIG  ← 距离 SM_CXICON  最近的条目(任务栏 / Alt-Tab 用)
//   - ICON_SMALL ← 距离 SM_CXSMICON 最近的条目(标题栏 / 任务管理器用)
#[cfg(windows)]
const APP_ICO_BYTES: &[u8] = include_bytes!("../icons/icon.ico");

/// 在 ICO 字节流中按目标像素尺寸挑选最匹配的条目,返回其原始位图数据切片(可直接交给
/// CreateIconFromResourceEx)。距离相同时偏好更大的条目,避免在小屏幕上被选到 16x16。
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
        // 排序 key:先按距离最小,再按尺寸最大(同距离取大,例如 want=32 时 32 优于 16)
        let key = (-dist, side);
        if best.map_or(true, |(d, s, _, _)| (key.0, key.1) > (d, s)) {
            best = Some((key.0, key.1, off, size));
        }
    }
    best.map(|(_, _, off, size)| &ico[off..off + size])
}

/// 给指定 HWND 重设 ICON_BIG / ICON_SMALL 为高分辨率版本。
/// 注意:WM_SETICON 设置后图标的销毁由窗口生命周期负责,这里设完不能 DestroyIcon
/// (否则系统再次绘制时拿到悬空句柄会画出空白/黑块)。
#[cfg(windows)]
fn apply_high_res_icons(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateIconFromResourceEx, GetSystemMetrics, SendMessageW, ICON_BIG, ICON_SMALL,
        LR_DEFAULTCOLOR, SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, WM_SETICON,
    };

    // Tauri 依赖的 windows 0.61 中 HWND 是 *mut c_void 的 newtype,与 windows-sys 0.59
    // 的 HWND(同样是 *mut c_void)二进制布局一致,直接 as 转即可。
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
            // 第四参数 dwVer=0x00030000 是 Windows 3.0+ 标准格式标记,固定值。
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
/// 这样用户看到的第一帧就是渲染好的 splash / 主界面,不再经历"白屏 → 紫屏"的闪烁。
///
/// 同时在这里再调一次 apply_high_res_icons —— 修复 Tauri runtime 注入 16x16 RGBA
/// 导致任务栏图标模糊的问题(详见 apply_high_res_icons 上方注释)。setup 已经设过
/// 一次,这里再设一次是为了覆盖某些时序下 wry 可能在窗口 show 之前重新写入低分图标
/// 的极端情况,确保用户看到的第一帧任务栏图标就是高分辨率版本。
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
/// reqwest 默认会通过代理转发请求,但代理通常不允许中转 127.0.0.1,
/// 导致 POST /start、POST /readsubscriptions 全失败 → 测试全 N/A。
/// 这是之前所有"测试无法跑"问题的真正根因。
fn local_backend_client() -> &'static reqwest::Client {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .expect("无法创建 local backend client")
    })
}

/// 通过 Rust 侧 reqwest 代理向后端发起请求,绕过 webview 的 mixed-content / CORS 限制
/// 以及系统代理拦截 127.0.0.1 的问题。统一走 invoke + .no_proxy() 是最稳的做法。
#[tauri::command]
async fn api_get(path: String) -> Result<String, String> {
    let url = format!("{}{}", BACKEND_URL, path);
    let res = local_backend_client()
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = res.status();
    let text = res.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("{} {}: {}", path, status.as_u16(), text));
    }
    Ok(text)
}

#[tauri::command]
async fn api_post_json(path: String, body: String) -> Result<String, String> {
    let url = format!("{}{}", BACKEND_URL, path);
    let res = local_backend_client()
        .post(&url)
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = res.status();
    let text = res.text().await.map_err(|e| e.to_string())?;
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
    let res = local_backend_client()
        .post(&url)
        .multipart(form)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = res.status();
    let text = res.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("{} {}: {}", path, status.as_u16(), text));
    }
    Ok(text)
}

/// 选定本地配置文件后,由 Rust 端一步完成:读取文件字节 → multipart 上传到后端
/// /readfileconfig → 返回后端响应文本(节点 JSON 数组,或 "error"/"running")。
///
/// 为什么不在前端做:Tauri webview 打包模式下 JS 侧读文件 + 多次 invoke 往返
/// (read_file_base64 → atob → api_post_file)任一环出错都难以察觉,且经过最
/// 脆弱的链路。收敛到 Rust 单命令后,文件读取与上传全程可控,任何失败都通过
/// Result::Err 冒泡到前端显示,不再"点了没反应"。
#[tauri::command]
async fn import_config_file(path: String) -> Result<String, String> {
    let bytes = std::fs::read(&path).map_err(|e| format!("读取文件失败 {path}: {e}"))?;
    let file_name = std::path::Path::new(&path)
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
    if let Some(mut c) = state.child.lock().unwrap().take() {
        kill_backend_tree(&mut c);
    }
    // 给 OS 一点时间清理端口/句柄
    std::thread::sleep(std::time::Duration::from_millis(400));
    // 重启前先做完整性检查:如果用户的 runtime engine 缺资产(tools/ 子树丢失等),
    // 这里会触发重新同步,从而让"设置 → 重启后端"成为通用自愈入口。
    ensure_runtime_engine(&app)?;
    let new_child = spawn_backend(&app)?;
    *state.child.lock().unwrap() = Some(new_child);
    Ok(())
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
fn read_file_base64(path: String) -> Result<String, String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let bytes = std::fs::read(&path).map_err(|e| format!("读文件失败 {path}: {e}"))?;
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

/// 列出 engine/logs/ 下的所有 .log 文件,按修改时间倒序(最新在前)。
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
/// 只返回文件尾部最多 max_kb KB —— 日志关心最新内容,超大文件也不卡前端。
#[tauri::command]
fn read_log_text(path: String, max_kb: Option<u64>) -> Result<String, String> {
    let cap = max_kb.unwrap_or(512) * 1024;
    let bytes = std::fs::read(&path).map_err(|e| format!("读日志失败 {path}: {e}"))?;
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
    let dir = engine_dir(&app)?.join("results");
    for ext in ["log", "png"] {
        let p = dir.join(format!("{name}.{ext}"));
        if p.exists() {
            std::fs::remove_file(&p).map_err(|e| format!("删除 {} 失败: {e}", p.display()))?;
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
        let dst = dst_dir.join(format!("{base}.{ext}"));
        std::fs::copy(&src, &dst).map_err(|e| format!("复制失败 {}: {e}", dst.display()))?;
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
/// translateIsp() 保持一致的关键词映射,前后端展示文案统一为中文。
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

/// 查询本机出口公网 IP 与地理位置/运营商。
/// 用 ip-api.com 中文接口:返回 IPv4 出口的 country / regionName / city / isp。
/// 失败时返回空字段(让 UI 优雅降级)。
#[tauri::command]
async fn get_my_ip_info() -> Result<GeoIpInfo, String> {
    use std::net::{IpAddr, Ipv4Addr};
    let client = reqwest::Client::builder()
        // 强制本地 IPv4 套接字出网,避免 IPv6 接口被解析到 IPv6 节点
        .local_address(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
        .timeout(std::time::Duration::from_secs(8))
        .user_agent("Mozilla/5.0 StairSpeedtest")
        .build()
        .map_err(|e| e.to_string())?;
    let url = "http://ip-api.com/json/?lang=zh-CN&fields=country,regionName,city,isp,status,message";
    let res = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("查询位置失败: {e}"))?;
    let json: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
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

/// 下载最新 mihomo 内核覆盖 runtime engine 下的二进制(只动 user 可写目录,不动 bundle)。
/// 资产按当前 OS+arch 选 zip(Windows)或 gz(Linux/macOS),失败时用 .bak 还原。
#[derive(Serialize)]
struct UpdateResult {
    success: bool,
    new_version: String,
    error: String,
}

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

    // 与 desktop.yml CI 装包时的资产命名保持一致,避免初装版与更新后版本不同源
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
            504 => "(GitHub 资产域被中间网关超时,请确认系统代理已开启)",
            403 => "(GitHub 速率限制或资产临时不可用,稍后重试)",
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

    // 文件被占用就无法覆盖写,先停掉在跑的 mihomo
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
    // macOS 首次执行会被 Gatekeeper 拦,清掉下载来源标记
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Linux 上 webkit2gtk 4.1 (2.46+) 在 Ubuntu 25.04 / Fedora 40+ / Wayland / 部分
    // NVIDIA / 老 Intel iGPU 等环境组合下,默认开启的 DMABUF 渲染器 + GPU 合成存在
    // 大量已知 bug,典型表现就是窗口黑屏 / 白屏 / 完全不显示(用户视角:"双击图标
    // 没反应")。社区有两条非常成熟的 workaround,都是关掉对应渲染路径回退到软件
    // 渲染,代价仅是少量性能损失,稳定性显著提升。相关 tauri-apps/tauri issue:
    //   #5143  Ubuntu blank screen → WEBKIT_DISABLE_COMPOSITING_MODE=1
    //   #13183 Manjaro NVIDIA 黑屏 → WEBKIT_DISABLE_DMABUF_RENDERER=1
    //   #15050 Fedora 43 + Sway 黑屏(Wayland)
    //   #13414 Ubuntu 25.04 GTK backend 初始化失败
    //
    // 这两条变量必须在创建 webview 前设好(Tauri::Builder 一旦初始化 webkit 就晚了),
    // 所以放在 run() 最前面。仅 Linux 设置;其他平台不受影响。
    // 用户已在环境里手动设置过任一变量时,std::env::set_var 仍幂等覆盖为 "1",
    // 这与社区文档的推荐值一致,不会破坏用户的自定义。
    #[cfg(target_os = "linux")]
    {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(BackendState {
            child: Mutex::new(None),
        })
        .setup(|app| {
            // 启动前清理可能残留的孤儿进程(尤其是 mihomo 内核),
            // 防止端口/句柄被占用导致新后端测试中途异常退出。
            cleanup_orphans();
            let handle = app.handle().clone();
            // 关键:先把打包的 engine 同步到用户可写位置,避免 Program Files 写权限问题
            if let Err(e) = ensure_runtime_engine(&handle) {
                eprintln!("[engine] 同步运行时目录失败: {e}");
            }
            // 写启动横幅到 engine/logs/app-startup.log,方便排查"装的是哪版"
            write_startup_banner(&handle);
            // 后台跑 .no_proxy() 自检,把结果写到 app-startup.log
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
                    *state.child.lock().unwrap() = Some(c);
                }
                Err(e) => eprintln!("[backend] 启动失败: {e}"),
            }
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
                let state = handle.state::<BackendState>();
                if let Some(mut c) = state.child.lock().unwrap().take() {
                    kill_backend_tree(&mut c);
                };
            }
        });
}
