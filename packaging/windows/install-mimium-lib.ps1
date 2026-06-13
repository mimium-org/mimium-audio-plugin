$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Test-HasContent {
    param([Parameter(Mandatory = $true)][string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Container)) {
        return $false
    }

    return $null -ne (Get-ChildItem -LiteralPath $Path -Force -ErrorAction SilentlyContinue | Select-Object -First 1)
}

function Invoke-InstallMimiumLibrary {
    $home = [Environment]::GetFolderPath('UserProfile')
    if ([string]::IsNullOrWhiteSpace($home) -or -not (Test-Path -LiteralPath $home)) {
        Write-Host '[mimium installer] user profile not found; skipping library bootstrap'
        return
    }

    $libDir = Join-Path $home '.mimium\lib'
    if (Test-HasContent -Path $libDir) {
        Write-Host "[mimium installer] library already exists at $libDir; skipping"
        return
    }

    New-Item -ItemType Directory -Force -Path $libDir | Out-Null

    $tmpDir = Join-Path ([IO.Path]::GetTempPath()) ("mimium-lib-" + [Guid]::NewGuid().ToString('N'))
    $extractDir = Join-Path $tmpDir 'extract'
    $archivePath = Join-Path $tmpDir 'mimium-rs.tar.gz'
    New-Item -ItemType Directory -Force -Path $extractDir | Out-Null

    $headers = @{
        Accept = 'application/vnd.github+json'
        'User-Agent' = 'mimium-audio-plugin-installer'
    }

    try {
        $tarballUrl = $null
        try {
            $release = Invoke-RestMethod -Uri 'https://api.github.com/repos/mimium-org/mimium-rs/releases/latest' -Headers $headers -ErrorAction Stop
            if ($null -ne $release -and -not [string]::IsNullOrWhiteSpace($release.tarball_url)) {
                $tarballUrl = $release.tarball_url
            }
        } catch {
            # Fall back to the main branch tarball endpoint.
        }

        if ([string]::IsNullOrWhiteSpace($tarballUrl)) {
            $tarballUrl = 'https://api.github.com/repos/mimium-org/mimium-rs/tarball/main'
        }

        Invoke-WebRequest -Uri $tarballUrl -Headers $headers -OutFile $archivePath -UseBasicParsing -ErrorAction Stop
        & tar -xzf $archivePath -C $extractDir
        if ($LASTEXITCODE -ne 0) {
            throw 'failed to extract downloaded mimium-rs tarball'
        }

        $sourceLib = Get-ChildItem -Path $extractDir -Recurse -Directory -Filter lib |
            Where-Object { $_.FullName -match 'mimium-lang[\\/]lib$' } |
            Select-Object -First 1

        if ($null -eq $sourceLib) {
            throw 'mimium-lang/lib directory was not found in downloaded archive'
        }

        Copy-Item -Path (Join-Path $sourceLib.FullName '*') -Destination $libDir -Recurse -Force
        Write-Host "[mimium installer] installed mimium library into $libDir"
    } catch {
        Write-Host "[mimium installer] failed to install mimium library: $($_.Exception.Message)"
    } finally {
        Remove-Item -Path $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Invoke-InstallMimiumLibrary
