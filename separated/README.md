# separated — 桌面端与历史剥离代码

本目录存放 **Node Speedtest** 桌面应用及相关剥离代码。

## 目录

| 路径 | 说明 |
|------|------|
| `desktop/` | Tauri + React 桌面应用；测速由进程内 Rust 引擎完成 |
| `desktop/src-tauri/engine/` | 运行时资产：mihomo、字体、pref.ini（不含旧 C++ sidecar） |
| `src/` | 历史 Web 服务源码（libevent），桌面端已不再依赖 |

## 桌面架构

```
React UI → Tauri invoke → Rust engine → mihomo (SOCKS5 / Clash API)
```

- 前端仍使用 `/readsubscriptions`、`/start`、`/getresults` 等路径名，由 Rust 进程内路由实现。
- 不再启动外部 C++ Web 服务进程。

## 构建

```bash
cd separated/desktop
npm install
pwsh -File scripts/sync-engine.ps1
npm run tauri build
```
