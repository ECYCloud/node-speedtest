# Node Speedtest

**由 mihomo (Clash.Meta) 内核驱动的现代化代理节点测速工具**

[![验证编译](https://github.com/ECYCloud/node-speedtest/actions/workflows/verify.yml/badge.svg)](https://github.com/ECYCloud/node-speedtest/actions/workflows/verify.yml)
[![GitHub release](https://img.shields.io/github/release/ECYCloud/node-speedtest.svg)](https://github.com/ECYCloud/node-speedtest/releases)
[![GitHub license](https://img.shields.io/github/license/ECYCloud/node-speedtest.svg)](https://github.com/ECYCloud/node-speedtest/blob/master/LICENSE)

## 简介

Node Speedtest 是面向桌面端的代理节点批量测速应用：

- **桌面壳**：Tauri 2 + React 18 + TypeScript + Tailwind
- **测速引擎**：进程内 Rust 异步编排（延迟中位数、预热丢弃、EMA 带宽估计、稳定提前结束）
- **代理内核**：内置 [mihomo (Clash.Meta)](https://github.com/MetaCubeX/mihomo)，负责协议解析与出站切换
- **能力**：订阅/Clash YAML/分享链接导入、全量或仅延迟测试、GeoIP、结果导出与历史记录

仓库内另保留可选的 CLI 构建目标（`src/`），与桌面应用解耦；日常使用以桌面安装包为准。

## 安装

### 预编译版本

访问 [发布页面](https://github.com/ECYCloud/node-speedtest/releases) 下载对应平台安装包。

### 桌面端开发构建

```bash
cd separated/desktop
npm install
# 同步 mihomo / 字体等到 src-tauri/engine/
pwsh -File scripts/sync-engine.ps1
npm run tauri dev
```

打包：

```bash
cd separated/desktop
npm run tauri build
```

## 测速逻辑（桌面引擎）

| 项目 | 说明 |
|------|------|
| 延迟 | mihomo `/proxies/*/delay` 多次采样取中位数；失败时 SOCKS5 HEAD 兜底 |
| 带宽 | 多连接经本地 SOCKS5 下载；丢弃预热窗；EMA 平滑；CV 稳定后提前结束 |
| 出站 | 单次启动 mihomo，按节点切换 `GLOBAL` selector |
| 停止 | 协作式取消，可在采样周期内响应 |

## 许可证

本软件：**MIT**（见 [LICENSE](LICENSE)）。

随包 [mihomo](https://github.com/MetaCubeX/mihomo) 为独立可执行文件，沿用其原许可证；说明见 [NOTICE](NOTICE)。
