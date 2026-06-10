param(
    [Parameter(Mandatory = $true)]
    [string]$Version
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = (Resolve-Path (Join-Path $scriptDir '..\..')).Path
$packageDir = Join-Path $repoRoot 'target\package\release'
$outDir = Join-Path $repoRoot 'target\installer\windows'
$stageRoot = Join-Path $outDir 'root'
$templateWxs = Join-Path $scriptDir 'installer.wxs'
$harvestWxs = Join-Path $outDir 'harvested.wxs'
$msiPath = Join-Path $outDir ("Mimium-Audio-Plugin-$Version-Windows.msi")

Remove-Item $stageRoot -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item $harvestWxs -Force -ErrorAction SilentlyContinue
Remove-Item $msiPath -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path (Join-Path $stageRoot 'CLAP') | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $stageRoot 'VST3') | Out-Null
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

$clapArtifact = Get-ChildItem -Path $packageDir -Filter '*.clap' | Select-Object -First 1
$vst3Artifact = Get-ChildItem -Path $packageDir -Filter '*.vst3' -Directory | Select-Object -First 1

if (-not $clapArtifact) {
    throw "Could not find a .clap artifact in $packageDir"
}

if (-not $vst3Artifact) {
    throw "Could not find a .vst3 artifact in $packageDir"
}

Copy-Item -Path $clapArtifact.FullName -Destination (Join-Path $stageRoot 'CLAP') -Recurse -Force
Copy-Item -Path $vst3Artifact.FullName -Destination (Join-Path $stageRoot 'VST3') -Recurse -Force

$heat = (Get-Command heat.exe).Source
$candle = (Get-Command candle.exe).Source
$light = (Get-Command light.exe).Source

& $heat dir $stageRoot -nologo -gg -srd -platform x64 -cg PluginFiles -dr PluginInstallRoot -var var.StagingRoot -out $harvestWxs
if ($LASTEXITCODE -ne 0) {
    throw 'heat.exe failed'
}

# heat.exe may emit 32-bit components even with -platform x64.
# Mark harvested components explicitly as Win64 to satisfy ICE80 checks.
[xml]$harvestDoc = Get-Content -Path $harvestWxs
$ns = New-Object System.Xml.XmlNamespaceManager($harvestDoc.NameTable)
$ns.AddNamespace('w', 'http://schemas.microsoft.com/wix/2006/wi')
$components = $harvestDoc.SelectNodes('//w:Component', $ns)
foreach ($component in $components) {
    $component.SetAttribute('Win64', 'yes')
}
$harvestDoc.Save($harvestWxs)

$candleArgs = @(
    '-nologo'
    "-dProductVersion=$Version"
    "-dStagingRoot=$stageRoot"
    '-out'
    "$outDir\"
    $templateWxs
    $harvestWxs
)

& $candle @candleArgs
if ($LASTEXITCODE -ne 0) {
    throw 'candle.exe failed'
}

$templateObj = Join-Path $outDir 'installer.wixobj'
$harvestObj = Join-Path $outDir 'harvested.wixobj'

& $light -nologo -sice:ICE61 -out $msiPath $templateObj $harvestObj
if ($LASTEXITCODE -ne 0) {
    throw 'light.exe failed'
}

Write-Host "Built MSI: $msiPath"
