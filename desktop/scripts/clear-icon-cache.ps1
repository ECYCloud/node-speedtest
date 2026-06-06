# Clear Windows Explorer icon cache so freshly installed/updated apps show
# the new icon on desktop and shortcuts (instead of a stale cached image).
#
# How it works:
#   Win10/11 stores icon cache at %LocalAppData%\IconCache.db and
#   %LocalAppData%\Microsoft\Windows\Explorer\iconcache_*.db
#   The files are locked by explorer.exe, so this script:
#     1. Stops explorer.exe
#     2. Deletes iconcache_*.db and thumbcache_*.db
#     3. Restarts explorer.exe
#
# Usage:
#   powershell -NoProfile -ExecutionPolicy Bypass -File desktop\scripts\clear-icon-cache.ps1
#
# Note: the desktop will briefly disappear (~1s) while explorer restarts.

[CmdletBinding()]
param(
    [switch]$Quiet
)

function Write-Step($msg) {
    if (-not $Quiet) { Write-Host "[icon-cache] $msg" }
}

$local = $env:LOCALAPPDATA
if (-not $local) {
    throw "LOCALAPPDATA is not set; cannot locate icon cache."
}

$paths = @(
    Join-Path $local 'IconCache.db'
) + (Get-ChildItem -Path (Join-Path $local 'Microsoft\Windows\Explorer') -Filter 'iconcache_*.db' -ErrorAction SilentlyContinue).FullName `
  + (Get-ChildItem -Path (Join-Path $local 'Microsoft\Windows\Explorer') -Filter 'thumbcache_*.db' -ErrorAction SilentlyContinue).FullName

$paths = $paths | Where-Object { $_ -and (Test-Path -LiteralPath $_) }

if (-not $paths) {
    Write-Step "No cache files found; system is already clean."
    Write-Step "If desktop icons still look stale, restart explorer.exe manually."
    return
}

Write-Step ("Will clear {0} cache file(s)" -f $paths.Count)

# 1) Stop explorer to release file locks
Write-Step "Stopping explorer.exe ..."
Get-Process -Name explorer -ErrorAction SilentlyContinue | ForEach-Object {
    $_ | Stop-Process -Force -ErrorAction SilentlyContinue
}
Start-Sleep -Milliseconds 800

# 2) Remove cache files
$removed = 0
$failed = 0
foreach ($p in $paths) {
    try {
        Remove-Item -LiteralPath $p -Force -ErrorAction Stop
        $removed++
        if (-not $Quiet) { Write-Host "  removed: $p" }
    }
    catch {
        $failed++
        Write-Warning ("  cannot delete: {0} ({1})" -f $p, $_.Exception.Message)
    }
}

# 3) Bring explorer back (it rebuilds caches automatically)
Write-Step "Starting explorer.exe ..."
Start-Process -FilePath 'explorer.exe'

Write-Step ("Done: removed {0}, failed {1}" -f $removed, $failed)
Write-Step "If desktop icons still look stale, delete the shortcut and recreate it, or sign out / reboot."
