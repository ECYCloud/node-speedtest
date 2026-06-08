# 从 EXE 文件的资源段提取所有 RT_BITMAP(NSIS header/sidebar 图就在这里)。
# 用法: .\extract-bmp-resource.ps1 <exe-path> <out-dir>
param([string]$ExePath, [string]$OutDir)

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.IO;
public class Res {
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern IntPtr LoadLibraryEx(string p, IntPtr r, uint f);
    [DllImport("kernel32.dll")] public static extern bool FreeLibrary(IntPtr h);
    public delegate bool EnumResNameProc(IntPtr h, IntPtr t, IntPtr n, IntPtr l);
    [DllImport("kernel32.dll", CharSet=CharSet.Auto)]
    public static extern bool EnumResourceNames(IntPtr h, IntPtr t, EnumResNameProc cb, IntPtr l);
    [DllImport("kernel32.dll", CharSet=CharSet.Auto)]
    public static extern IntPtr FindResource(IntPtr h, IntPtr name, IntPtr type);
    [DllImport("kernel32.dll")] public static extern IntPtr LoadResource(IntPtr h, IntPtr r);
    [DllImport("kernel32.dll")] public static extern IntPtr LockResource(IntPtr d);
    [DllImport("kernel32.dll")] public static extern uint SizeofResource(IntPtr h, IntPtr r);
    public const uint LOAD_LIBRARY_AS_DATAFILE = 0x00000002;
    public const int RT_BITMAP = 2;
}
"@

if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir | Out-Null }

$h = [Res]::LoadLibraryEx((Resolve-Path $ExePath).Path, [IntPtr]::Zero, [Res]::LOAD_LIBRARY_AS_DATAFILE)
if ($h -eq [IntPtr]::Zero) { Write-Host "LoadLibraryEx 失败: $(Get-Item $ExePath)"; exit 1 }

$names = New-Object System.Collections.Generic.List[IntPtr]
$cb = [Res+EnumResNameProc] {
  param($hh, $tt, $nn, $ll)
  $names.Add($nn)
  return $true
}
[Res]::EnumResourceNames($h, [IntPtr][Res]::RT_BITMAP, $cb, [IntPtr]::Zero) | Out-Null

Write-Host "$(Split-Path $ExePath -Leaf): 找到 $($names.Count) 个 BMP 资源"

foreach ($n in $names) {
  $hres = [Res]::FindResource($h, $n, [IntPtr][Res]::RT_BITMAP)
  if ($hres -eq [IntPtr]::Zero) { continue }
  $hg = [Res]::LoadResource($h, $hres)
  $size = [Res]::SizeofResource($h, $hres)
  $ptr = [Res]::LockResource($hg)
  $buf = New-Object byte[] $size
  [Runtime.InteropServices.Marshal]::Copy($ptr, $buf, 0, [int]$size)

  # RT_BITMAP 资源体里只有 BITMAPINFOHEADER 之后的内容,需要补 14 字节 BITMAPFILEHEADER
  # 解析 DIB header 算 pixel data offset
  $hdrSize = [BitConverter]::ToUInt32($buf, 0)
  $w = [BitConverter]::ToInt32($buf, 4)
  $hh = [BitConverter]::ToInt32($buf, 8)
  $bpp = [BitConverter]::ToUInt16($buf, 14)
  $compression = [BitConverter]::ToUInt32($buf, 16)
  $colorsUsed = [BitConverter]::ToUInt32($buf, 32)

  # 计算调色板大小(8bpp 以下才有)
  $paletteSize = 0
  if ($bpp -le 8) {
    $colors = if ($colorsUsed -eq 0) { [Math]::Pow(2, $bpp) } else { $colorsUsed }
    $paletteSize = $colors * 4
  }
  # 位掩码(BI_BITFIELDS)
  $maskSize = if ($compression -eq 3) { 12 } else { 0 }

  $pixelOffset = 14 + $hdrSize + $paletteSize + $maskSize
  $fileSize = 14 + $size
  $bmp = New-Object byte[] $fileSize
  $bmp[0] = 0x42; $bmp[1] = 0x4D
  [Array]::Copy([BitConverter]::GetBytes([uint32]$fileSize), 0, $bmp, 2, 4)
  [Array]::Copy([BitConverter]::GetBytes([uint32]$pixelOffset), 0, $bmp, 10, 4)
  [Array]::Copy($buf, 0, $bmp, 14, $size)

  $name = if ($n.ToInt64() -lt 65536) { "id$($n.ToInt64())" } else { [Marshal]::PtrToStringAuto($n) }
  $outPath = Join-Path $OutDir "$name-${w}x${hh}-${bpp}bpp.bmp"
  [IO.File]::WriteAllBytes($outPath, $bmp)
  Write-Host "  → $name : ${w}×${hh} ${bpp}bpp ($size bytes payload) → $outPath"
}

[Res]::FreeLibrary($h) | Out-Null
