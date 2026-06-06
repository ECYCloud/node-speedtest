$ErrorActionPreference = 'Continue'
$env:PATH = 'C:\msys64\mingw64\bin;' + $env:PATH
$gpp = 'C:\msys64\mingw64\bin\g++.exe'
$root = 'd:\Offices\stairspeedtest-reborn-master'
$src = Join-Path $root 'test\utf8probe.cpp'
$out = Join-Path $root 'test\utf8probe.exe'
$inc = @('-IC:/msys64/mingw64/include/libpng16', '-isystem', 'C:/msys64/mingw64/include/freetype2')
$libs = @('C:/msys64/mingw64/lib/libPNGwriter.a','C:/msys64/mingw64/lib/libpng.dll.a',
          'C:/msys64/mingw64/lib/libz.dll.a','C:/msys64/mingw64/lib/libfreetype.dll.a',
          '-lharfbuzz')
$args = @('-std=gnu++17','-O2','-DUSE_FREETYPE') + $inc + @($src,'-o',$out) + $libs
Write-Host 'compiling utf8probe...'
& $gpp @args 2>&1 | Select-Object -First 10
Write-Host ("compile exit=" + $LASTEXITCODE)
if ($LASTEXITCODE -eq 0) {
    Push-Location (Join-Path $root 'stairspeedtest-mihomo-win64')
    # 确保 results 目录存在
    New-Item -ItemType Directory -Force -Path 'results' | Out-Null
    & $out
    Write-Host ("run exit=" + $LASTEXITCODE)
    $png = Join-Path (Join-Path $root 'stairspeedtest-mihomo-win64') 'results\utf8probe.png'
    if (Test-Path $png) { Write-Host ("PNG: " + $png + "  " + (Get-Item $png).Length + " bytes") }
    Pop-Location
}
