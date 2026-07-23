# 同步桌面端运行时资产（仅 mihomo + 字体 + 配置）
#
# 不再同步 stairspeedtest.exe / C++ DLL：测速由 Tauri 进程内 Rust 引擎完成。
#
# 用法:
#   pwsh -File separated/desktop/scripts/sync-engine.ps1

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path "$PSScriptRoot\..\..\.."
$srcRoot = Join-Path $repoRoot "stairspeedtest-mihomo-win64"
$dst = Join-Path $repoRoot "separated\desktop\src-tauri\engine"

if (-not (Test-Path $srcRoot)) {
    throw "源目录不存在: $srcRoot"
}

New-Item -ItemType Directory -Force -Path (Join-Path $dst "tools\clients") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $dst "tools\misc") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $dst "logs") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $dst "results") | Out-Null

$mihomoSrc = Join-Path $srcRoot "tools\clients\mihomo.exe"
if (-not (Test-Path $mihomoSrc)) {
    $mihomoSrc = Join-Path $srcRoot "tools\clients\mihomo"
}
if (-not (Test-Path $mihomoSrc)) {
    throw "找不到 mihomo 二进制"
}
Copy-Item -Force $mihomoSrc (Join-Path $dst "tools\clients\")

$font = Join-Path $srcRoot "tools\misc\SourceHanSansCN-Medium.otf"
if (Test-Path $font) {
    Copy-Item -Force $font (Join-Path $dst "tools\misc\")
}
$emoji = Join-Path $srcRoot "tools\misc\TwemojiFlat.ttf"
if (Test-Path $emoji) {
    Copy-Item -Force $emoji (Join-Path $dst "tools\misc\")
}
# 禁止残留硬编码国旗 PNG；国旗仅由 TwemojiFlat.ttf 运行时识别渲染
Remove-Item -Recurse -Force (Join-Path $dst "tools\misc\twemoji") -ErrorAction SilentlyContinue

# Material Icons（结果图 TLS 状态）：从仓库本地 SVG 栅格化，禁止外链/CDN
$raster = Join-Path $PSScriptRoot "rasterize-material-icons.mjs"
if (Test-Path $raster) {
    Push-Location (Split-Path $PSScriptRoot -Parent)
    try {
        node $raster
    } finally {
        Pop-Location
    }
}

$pref = Join-Path $srcRoot "pref.ini"
if (-not (Test-Path $pref)) {
    $pref = Join-Path $repoRoot "base\pref.ini"
}
if (Test-Path $pref) {
    Copy-Item -Force $pref (Join-Path $dst "pref.ini")
}

$ca = Join-Path $srcRoot "cacert.pem"
if (Test-Path $ca) {
    Copy-Item -Force $ca (Join-Path $dst "cacert.pem")
}

# 清除旧 C++ sidecar 与仅为其服务的 DLL
Get-ChildItem $dst -File | Where-Object {
    $_.Name -match '^(stairspeedtest|lib.*\.(dll|so)|zlib1\.dll)' -or
    $_.Extension -in '.dll', '.so'
} | ForEach-Object {
    Remove-Item -Force $_.FullName -ErrorAction SilentlyContinue
}
Remove-Item -Force (Join-Path $dst "stairspeedtest.exe") -ErrorAction SilentlyContinue
Remove-Item -Force (Join-Path $dst "stairspeedtest") -ErrorAction SilentlyContinue

Write-Host "✓ 引擎资产已同步（仅 mihomo/字体/配置）" -ForegroundColor Green
