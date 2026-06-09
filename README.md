# 阶梯测速 · 重生版
**由 mihomo (Clash.Meta) 内核驱动的代理批量测速工具**
[![验证编译](https://github.com/ECYCloud/stairspeedtest-reborn-mihomo/actions/workflows/verify.yml/badge.svg)](https://github.com/ECYCloud/stairspeedtest-reborn-mihomo/actions/workflows/verify.yml)
[![GitHub tag (latest SemVer)](https://img.shields.io/github/tag/ECYCloud/stairspeedtest-reborn-mihomo.svg)](https://github.com/ECYCloud/stairspeedtest-reborn-mihomo/tags)
[![GitHub release](https://img.shields.io/github/release/ECYCloud/stairspeedtest-reborn-mihomo.svg)](https://github.com/ECYCloud/stairspeedtest-reborn-mihomo/releases)
[![GitHub license](https://img.shields.io/github/license/ECYCloud/stairspeedtest-reborn-mihomo.svg)](https://github.com/ECYCloud/stairspeedtest-reborn-mihomo/blob/master/LICENSE)

## 简介
本项目是原始 [阶梯测速](https://github.com/tindy2013/stairspeedtest-reborn) 脚本的 C++ 重写版本。相比脚本版本,本项目具有更快的节点解析速度、更精美的结果图片渲染,以及完整的跨平台支持。代理后端已统一集成到单一的内置 [mihomo (Clash.Meta)](https://github.com/MetaCubeX/mihomo) 内核中,因此每种支持的协议都由一个二进制文件驱动,而非一堆分散的协议客户端。

## 特别感谢
* [@NyanChanMeow](https://github.com/nyanchanmeow) - [SSRSpeed](https://github.com/tindy2013/stairspeedtest-reborn) 原始脚本的作者
* [@MetaCubeX](https://github.com/MetaCubeX) - [mihomo](https://github.com/MetaCubeX/mihomo) 代理内核项目
* [@CareyWong](https://github.com/careywang) - Web 界面设计
* [@ang830715](https://github.com/ang830715) - macOS 支持
* ...以及测试阶段所有帮助过我的人!
  
## 安装
### 预编译版本
访问 [发布页面](https://github.com/ECYCloud/stairspeedtest-reborn-mihomo/releases)。
### 从源代码编译
需要 CMake 3.10+ 和 C++17 编译器,以及以下库:

| 库名 | 用途 |
|------|------|
| libcurl | HTTP/HTTPS 请求 |
| openssl (≥1.1.0) | TLS/SSL 加密 |
| libevent | 事件驱动网络库 |
| libpcre2 | 正则表达式匹配 |
| rapidjson | 配置文件解析 |
| yaml-cpp | YAML 配置格式支持 |
| libpng | PNG 图像库 |
| freetype | 字体栅格化 |
| harfbuzz | 高级文字排版(彩色表情符、连字) |
| zlib, bzip2 | 压缩库 |
| PNGwriter | 高级 PNG 生成库 |

平台特定安装说明:

**Ubuntu / Debian:**
```bash
sudo apt-get install cmake pkg-config \
  libcurl4-openssl-dev libssl-dev libyaml-cpp-dev \
  libevent-dev libpcre2-dev rapidjson-dev \
  libpng-dev libfreetype-dev libharfbuzz-dev \
  zlib1g-dev libbz2-dev

# 从源代码编译 PNGwriter (标准仓库中没有)
git clone --depth 1 https://github.com/pngwriter/pngwriter.git
cd pngwriter && cmake -DCMAKE_INSTALL_PREFIX=/usr/local . && make install && cd ..
```

**macOS (Homebrew):**
```bash
brew install cmake pkg-config \
  curl openssl@3 yaml-cpp \
  libevent pcre2 rapidjson \
  libpng freetype harfbuzz zlib bzip2

# 从源代码编译 PNGwriter
git clone --depth 1 https://github.com/pngwriter/pngwriter.git
cd pngwriter && cmake -DCMAKE_INSTALL_PREFIX=$(brew --prefix) . && make install && cd ..
```

**Windows (MSYS2 MinGW64):**
```bash
# 在 MSYS2 shell 中执行,mingw-w64-x86_64 环境
pacman -S mingw-w64-x86_64-{cmake,pkg-config,curl,openssl,yaml-cpp,libevent,pcre2,rapidjson,libpng,freetype,harfbuzz,zlib,bzip2}

# 从源代码编译 PNGwriter
git clone --depth 1 https://github.com/pngwriter/pngwriter.git
cd pngwriter && cmake -DCMAKE_INSTALL_PREFIX=/mingw64 . && make install && cd ..
```

### 代理内核
本程序内置 [mihomo](https://github.com/MetaCubeX/mihomo) 代理内核来处理所有代理协议。在 **非 Windows 系统**上,必须在运行前将编译好的 `mihomo` 二进制文件放置在 `tools/clients/mihomo`。

### 编译
```bash
cmake -S . -B build -DCMAKE_BUILD_TYPE=Release
cmake --build build -j
./build/stairspeedtest        # Linux/macOS
./build/Release/stairspeedtest.exe  # Windows
```

## 使用方法
* **命令行模式(默认):** 运行二进制文件进入交互模式,从标准输入粘贴订阅链接或配置文件
* **Web 模式:** 在 `pref.ini` 中设置 `webserver_mode=true`,或使用 `--web` 标志启动 HTTP 服务器(监听 localhost:25500),然后在浏览器访问 `http://localhost:25500/web`
* 测速结果保存到 `results/` 文件夹,包含日志文件和 PNG 图片
* 通过编辑 `pref.ini` 自定义设置(测速模式、线程数、监听地址端口等)

## 兼容性

### 命令行 / Web 引擎(`stairspeedtest`)

CI 每次提交都跑,三平台都过编译与冒烟测试:

| 平台 | CPU | 操作系统 | 状态 |
|------|-----|---------|------|
| **Linux** | x86_64 | Ubuntu 24.04 | ✓ 已验证 |
| **Windows** | x86_64 | 最新版 | ✓ 已验证 |
| **macOS** | arm64 | 最新版 | ✓ 已验证 |

### Tauri 桌面端(`separated/desktop/`)

| 平台 | 安装包 | 状态 |
|------|--------|------|
| **Windows** x86_64 | NSIS (`.exe`) | ✓ 主分发渠道,长期验证 |
| **Linux** x86_64 | `.deb` / `.AppImage` | ⚠️ 实验性,见下面平台说明 |
| **macOS** arm64 | `.dmg`(未签名) | ⚠️ 实验性,首次启动需在 Gatekeeper 中放行 |

**Linux 选哪个包**:

| 发行版 | 推荐 | 原因 |
|--------|------|------|
| Ubuntu 22.04 / 24.04, Debian 12 | `.deb` | 系统自带 `libwebkit2gtk-4.1-0`,deb 体积小 |
| **Ubuntu 25.04+, Fedora 40+, Arch** | **`.AppImage`** | 这些发行版主仓库已移除 webkit2gtk-4.1(切到 6.0,Tauri 2 暂未适配),`.deb` 装上也起不来;AppImage 自带 webkit runtime |
| 其他 | 优先 AppImage | 自包含,无需考虑发行版差异 |

AppImage 用法:`chmod +x Stair.Speedtest_*.AppImage && ./Stair.Speedtest_*.AppImage`

本项目通过 **[传统 build.yml](https://github.com/ECYCloud/stairspeedtest-reborn-mihomo/actions/workflows/build.yml)** 为 Windows x86/x86_64 维护发布包(仅在打版本标签时触发)。这些构建包含完整的 mihomo 内核和运行时库。

曾支持的平台(不再纳入活跃 CI):
* Windows 7/2008 R2 x64
* CentOS 7.x, Debian 6.3+
* Raspberry Pi 4B (armv7l)
* Android 8+ (Termux)
* iOS/iPadOS (iSH shell)

## 支持的代理协议

所有协议均由内置的 [mihomo (Clash.Meta)](https://github.com/MetaCubeX/mihomo) 内核提供支持:

| 协议 | 支持的配置格式 | 状态 |
|------|------------------|------|
| **Shadowsocks (SS)** | Shadowsocks / ShadowsocksD / Clash / Surge 2+ / Quantumult(X) | ✓ 完整支持 |
| **ShadowsocksR (SSR)** | ShadowsocksR / Quantumult(X) / SSTap / Netch | ✓ 完整支持 |
| **VMess** | V2RayN / Clash / Quantumult(X) | ✓ 完整支持 |
| **VLESS** | Clash / Quantumult(X) / Surge 4+ | ✓ 完整支持 |
| **Trojan** | Trojan / Clash / Surge 4+ / Quantumult(X) | ✓ 完整支持 |
| **Hysteria v1** | Clash / Surge / Surfboard | ✓ 完整支持 |
| **Hysteria2 / hy2** | Clash / Surge / Surfboard | ✓ 完整支持 |
| **TUIC** | Clash / sing-box | ✓ 完整支持 |
| **AnyTLS** | Clash | ✓ 完整支持 |
| **SOCKS5** | Telegram / SSTap / Clash | ✓ 完整支持 |
| **HTTP / HTTPS** | Telegram / Clash | ✓ 完整支持 |

**支持的订阅/配置格式:**
* 直接代理链接(URI 协议)
* Clash YAML 配置
* V2RayN JSON (Base64 编码)
* Surge 配置(片段或完整文件)
* Quantumult(X) 配置
* SSTap (NTT) 配置
* Netch (sing-box JSON) 配置
* SSD 格式
* Shadowsocks Android JSON

## 已知问题
* 暂无

## 待办事项
* 暂无
