# 已从主项目剥离的非 CLI 客户端代码

本目录存放从 **Stair Speedtest Reborn** 主项目剥离出来的全部「非客户端」逻辑
（网页端 Web 服务、旧浏览器 SPA、Tauri 桌面应用）。

剥离原则：**主项目是纯 CLI mihomo 测速引擎，100% 独立，不依赖本目录任何代码。**
本目录的代码能否独立编译/运行，由各自后续维护者负责，与主项目无关。

## 目录内容

| 路径 | 原位置 | 说明 |
|------|--------|------|
| `src/webserver_libevent.cpp` | `src/` | 基于 libevent 的内置 HTTP 服务器实现 |
| `src/webserver.h` | `src/` | HTTP 服务器接口声明（Request/Response/路由注册） |
| `src/webgui_wrapper.cpp` | `src/` | Web 服务模式的 HTTP 路由与 JSON 序列化（前端的真正后端） |
| `base/webui/` | `base/` | 旧浏览器 SPA 静态资源（index.html / *.js） |
| `base/webgui.bat`、`webgui.sh` | `base/` | 旧 websocketd RPC GUI 启动脚本 |
| `base/webserver.bat`、`webserver.sh` | `base/` | `/web` 服务模式启动脚本 |
| `desktop/` | 仓库根 | Tauri + React + TypeScript 桌面应用 |

## 剥离前的依赖关系（已在主项目侧切断）

这些代码原先与主项目核心存在如下耦合，**剥离时已从主项目删除对应入口**：

### 1. `webgui_wrapper.cpp` → 主项目核心

`ssrspeed_webserver_routine()` 通过以下 `extern` 符号调用主项目逻辑：

- 全局变量：`allNodes`、`cur_node_id`、`socksport`、`speedtest_mode`、
  `export_sort_method`、`custom_group`、`override_conf_port`、
  `custom_exclude_remarks`、`custom_include_remarks`、`node_count`
- 函数：`addNodes()`、`rewriteNodeID()`、`batchTest()`、
  `launchMihomoForNodes()`、`explodeConfContent()`、`streamToInt()`

也就是说，这个文件**不是自包含的**：它需要链接进 `main.cpp` 等核心目标文件才能工作。
独立化需要把上述核心逻辑一并带出，或改为通过进程/IPC 调用主程序。

### 2. `webserver_libevent.cpp` → 主项目

依赖核心的 `misc.h`、`socket.h`、`logger.h`，以及 `extern std::string user_agent_str`。
对外提供 `start_web_server_multi()` / `append_response()` 等供 `webgui_wrapper.cpp` 使用。

### 3. 桌面端 `desktop/` → `/web` 服务模式

`desktop/src-tauri/src/lib.rs` 中：

- `BACKEND_URL = "http://127.0.0.1:10870"`
- `spawn_backend()` 启动 `stairspeedtest.exe /web`，通过该 HTTP API 与后端通信
- `sync-engine.ps1` 把 `stairspeedtest-mihomo-win64/`（含 mihomo 内核 + 引擎 exe + DLL）
  同步进 `desktop/src-tauri/engine/`

**主项目已删除 `/web` 参数与整个 webserver 模式**，因此当前主项目产出的
`stairspeedtest.exe` 不再响应 `/web`。桌面端若要复用，需要：

- 要么把本目录的 `webserver_libevent.cpp` + `webgui_wrapper.cpp` + 核心源文件
  组成一份独立的「引擎服务版」构建；
- 要么改造桌面端，改用直接调用 CLI 进程并解析其 stdout/结果文件的方式。

## 主项目侧已做的删除（对照）

- `CMakeLists.txt`：移除 `webgui_wrapper.cpp`、`webserver_libevent.cpp` 源文件，
  移除 `libevent`（`PKG_CHECK_MODULES LIBEVENT`）依赖。
- `src/main.cpp`：删除 `webserver_mode` / `listen_address` / `listen_port` 全局，
  删除 `/web` 命令行参数、`ssrspeed_webserver_routine` 前向声明与启动分支、
  `readConf` 的 `[webserver]` 段，以及 `singleTest` / `batchTest` / `addNodes`
  中所有 `webserver_mode` 条件分支。
- `scripts/build.windows.release.sh`：链接命令移除 `-levent`。

剥离后主项目已验证可独立配置、编译、产出 `stairspeedtest.exe`。
