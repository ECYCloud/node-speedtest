# 同步后端引擎到桌面工程
#
# 用途:把 stairspeedtest-mihomo-win64 内的可执行文件、DLL、mihomo 内核与
#       配置同步到 desktop/src-tauri/engine/,供 Tauri 在 dev 与打包时使用。
#
# 已剥离:
#   - webui/                       旧浏览器 SPA,Tauri 已完全取代
#   - tools/gui/                   旧浏览器 SPA 的 favicon/index.html
#   - tools/misc/WenQuanYiMicroHei-01.ttf  渲染器只用 SourceHanSansCN,该字体未加载
#   - webgui.bat / webserver.bat   浏览器/CLI 启动脚本
#   - logs/ / results/             运行时产物
#   - cache.db / *.bak             运行时缓存与备份
#
# 用法(在仓库根目录执行):
#   pwsh -File desktop/scripts/sync-engine.ps1

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path "$PSScriptRoot\..\.."
$src = Join-Path $repoRoot "stairspeedtest-mihomo-win64"
$dst = Join-Path $repoRoot "desktop\src-tauri\engine"

if (-not (Test-Path $src)) {
    throw "源目录不存在: $src"
}

New-Item -ItemType Directory -Force -Path $dst | Out-Null

Write-Host "→ 同步 $src" -ForegroundColor Cyan
Write-Host "  -> $dst" -ForegroundColor Cyan

# /E 递归(含空目录) /XD 排除目录 /XF 排除文件
# /R:1 /W:1 单次重试 短等待 /NFL /NDL 不打印每个文件 /NJH /NJS 静默
$rc = (Start-Process -FilePath robocopy `
    -ArgumentList @(
        "`"$src`"", "`"$dst`"",
        "/E",
        "/XD", "webui", "logs", "results", "gui",
        "/XF", "cache.db", "*.bak", "webgui.bat", "webserver.bat", "WenQuanYiMicroHei-01.ttf",
        "/R:1", "/W:1", "/NFL", "/NDL", "/NJH", "/NJS"
    ) `
    -NoNewWindow -Wait -PassThru).ExitCode

# robocopy 0~7 视为成功
if ($rc -ge 8) {
    throw "robocopy 失败,退出码 $rc"
}

Write-Host "✓ 同步完成 (robocopy 退出码 $rc)" -ForegroundColor Green
