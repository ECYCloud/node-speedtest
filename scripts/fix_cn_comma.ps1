param([switch]$DryRun = $false)
$cnComma = [regex]'(?<=[\u4e00-\u9fa5]),(?=\s*[\u4e00-\u9fa5])'
$replacement = [string][char]0xFF0C
$includeExt = @('.cpp', '.h', '.rs', '.tsx', '.ts', '.md', '.js', '.mjs', '.cjs')
$excludeDirs = @('node_modules','target','build','build-engine','build-web','dist','gen','.cargo','.git','cmake','include')
$root = (Get-Location).Path
$sep1 = [char]92
$sep2 = [char]47
$files = Get-ChildItem -Recurse -File -Path $root | Where-Object {
    $rel = $_.FullName.Substring($root.Length).TrimStart($sep1, $sep2)
    $ext = $_.Extension.ToLower()
    if ($includeExt -notcontains $ext) { return $false }
    foreach ($x in $excludeDirs) {
        $pattern = '(^|[\\/])' + [regex]::Escape($x) + '([\\/]|$)'
        if ($rel -match $pattern) { return $false }
    }
    return $true
}
$changed = 0; $totalReplacements = 0
foreach ($f in $files) {
    $orig = Get-Content -Raw -Encoding utf8 -LiteralPath $f.FullName
    if ($null -eq $orig) { continue }
    $matches2 = $cnComma.Matches($orig)
    if ($matches2.Count -eq 0) { continue }
    $new = $cnComma.Replace($orig, $replacement)
    $totalReplacements += $matches2.Count
    $changed++
    $rel = $f.FullName.Substring($root.Length).TrimStart($sep1, $sep2)
    Write-Host ("CHG {0,-60} ({1})" -f $rel, $matches2.Count) -ForegroundColor Yellow
    if (-not $DryRun) {
        $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
        [System.IO.File]::WriteAllText($f.FullName, $new, $utf8NoBom)
    }
}
Write-Host "---"
Write-Host ("Files: {0}, Replacements: {1}, DryRun: {2}" -f $changed, $totalReplacements, $DryRun)
