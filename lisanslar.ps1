# Exe'ye derlenen Rust kutuphanelerinin lisans dosyalarini "cargo metadata" ile toplar.
# Kullanim: powershell -ExecutionPolicy Bypass -File lisanslar.ps1 <hedef klasor> [hedef platform]
param([Parameter(Mandatory = $true)][string]$Hedef, [string]$Platform = 'x86_64-pc-windows-msvc')
$ErrorActionPreference = 'Stop'

# ConvertFrom-Json, Windows PowerShell 5.1'de buyuk JSON'da sinira takilir; bu yuzden .NET cozucusu
Add-Type -AssemblyName System.Web.Extensions
$cozucu = New-Object System.Web.Script.Serialization.JavaScriptSerializer
$cozucu.MaxJsonLength = [int]::MaxValue
$json = cargo metadata --format-version 1 --filter-platform $Platform | Out-String
if ($LASTEXITCODE -ne 0) { throw 'cargo metadata basarisiz' }
$veri = $cozucu.DeserializeObject($json)

# Yalnizca uygulamaya gercekten derlenen paketler (derleme betigi bagimliliklari haric)
$normal = @{}
$kuyruk = New-Object System.Collections.Queue
foreach ($n in $veri['resolve']['nodes']) { if ($n['id'] -eq $veri['resolve']['root']) { $kuyruk.Enqueue($n) } }
$dugumler = @{}
foreach ($n in $veri['resolve']['nodes']) { $dugumler[$n['id']] = $n }
while ($kuyruk.Count -gt 0) {
    $n = $kuyruk.Dequeue()
    foreach ($d in $n['deps']) {
        $calisma = $false
        foreach ($t in $d['dep_kinds']) { if ($null -eq $t['kind']) { $calisma = $true } }
        if ($calisma -and -not $normal.ContainsKey($d['pkg'])) { $normal[$d['pkg']] = $true; $kuyruk.Enqueue($dugumler[$d['pkg']]) }
    }
}

if (Test-Path -LiteralPath $Hedef) { Remove-Item -LiteralPath $Hedef -Recurse -Force }
New-Item -ItemType Directory -Force -Path $Hedef | Out-Null
$liste = @()
foreach ($p in ($veri['packages'] | Sort-Object { $_['name'] }, { $_['version'] })) {
    if ($null -eq $p['source']) { continue }  # uygulamanin kendisi
    if (-not $normal.ContainsKey($p['id'])) { continue }
    $dizin = Split-Path -Parent $p['manifest_path']
    $dosyalar = Get-ChildItem -LiteralPath $dizin -File | Where-Object { $_.Name -match '^(LICEN[CS]E|COPYING|NOTICE|UNLICENSE)' }
    if ($dosyalar) {
        $k = Join-Path $Hedef ($p['name'] + '-' + $p['version'])
        New-Item -ItemType Directory -Force -Path $k | Out-Null
        foreach ($f in $dosyalar) { Copy-Item -LiteralPath $f.FullName -Destination (Join-Path $k ($f.BaseName + '.txt')) }
    }
    $liste += ($p['name'] + ' ' + $p['version'] + "`t" + $p['license'])
}
$liste | Set-Content -Encoding UTF8 -Path (Join-Path $Hedef 'KUTUPHANELER.txt')
Write-Output ("{0} kutuphanenin lisansi toplandi." -f $liste.Count)
