use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Manager, RunEvent, State};

/// 后端进程统一监听地址,与 stairspeedtest pref.ini 的 [webserver] 默认值保持一致
const BACKEND_URL: &str = "http://127.0.0.1:10870";

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
/// 升级判定:对比 bundled 与已安装 stairspeedtest.exe 的 mtime+size。
/// 之前用 CARGO_PKG_VERSION 字符串戳判定 — 开发期版本号不变就永远不同步,
/// 用户必须"卸载 + 删数据"才能拿到最新二进制。改成文件指纹后,只要后端真的
/// 有变化就自动同步,不再依赖 0.1.0 → 0.1.1 这种语义化升级。
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
        let src_exe = src.join("stairspeedtest.exe");
        let dst_exe = dst.join("stairspeedtest.exe");

        // 取 (size, mtime) 作为指纹,任一不同就重新同步
        let fingerprint = |p: &std::path::Path| -> Option<(u64, std::time::SystemTime)> {
            let m = std::fs::metadata(p).ok()?;
            Some((m.len(), m.modified().ok()?))
        };
        if let (Some(a), Some(b)) = (fingerprint(&src_exe), fingerprint(&dst_exe)) {
            if a == b {
                return Ok(()); // 两侧 exe 完全一致,无需同步
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
    let exe = dir.join(if cfg!(windows) {
        "stairspeedtest.exe"
    } else {
        "stairspeedtest"
    });
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

#[cfg(not(windows))]
fn cleanup_orphans() {}

/// 在 Windows 上彻底清理后端进程及其子孙(mihomo 内核),避免孤儿进程。
/// 直接 child.kill() 在 Windows 上是 TerminateProcess,不会清理子进程。
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
        let _ = pid;
    }
}

#[tauri::command]
fn backend_url() -> &'static str {
    BACKEND_URL
}

/// 启动闪烁修复:tauri.conf.json 设了 visible:false,窗口启动时不可见,
/// 等前端 React 完成首屏渲染后调这个命令把主窗口显示出来 + 设置焦点。
/// 这样用户看到的第一帧就是渲染好的 splash / 主界面,不再经历"白屏 → 紫屏"的闪烁。
#[tauri::command]
fn show_main_window(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
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

#[tauri::command]
fn restart_backend(app: AppHandle, state: State<BackendState>) -> Result<(), String> {
    if let Some(mut c) = state.child.lock().unwrap().take() {
        kill_backend_tree(&mut c);
    }
    // 给 OS 一点时间清理端口/句柄
    std::thread::sleep(std::time::Duration::from_millis(400));
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

/// 导出一条历史:把 results/<name>.log 和 .png 复制到 target_dir,文件名前加 prefix
#[tauri::command]
fn export_history(
    app: AppHandle,
    name: String,
    target_dir: String,
    prefix: String,
) -> Result<Vec<String>, String> {
    let src_dir = engine_dir(&app)?.join("results");
    let dst_dir = std::path::PathBuf::from(&target_dir);
    if !dst_dir.is_dir() {
        return Err(format!("目标目录无效: {}", dst_dir.display()));
    }
    let mut written = Vec::new();
    let safe_prefix = prefix.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
    for ext in ["log", "png"] {
        let src = src_dir.join(format!("{name}.{ext}"));
        if !src.exists() {
            continue;
        }
        let fname = if safe_prefix.is_empty() {
            format!("{name}.{ext}")
        } else {
            format!("{safe_prefix}-{name}.{ext}")
        };
        let dst = dst_dir.join(&fname);
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
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
            restart_backend,
            list_history,
            read_file_base64,
            delete_history_item,
            clear_history,
            export_history,
            get_my_ip_info
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
