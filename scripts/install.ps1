param(
    [string]$Version = "latest",
    [string]$InstallDir = ""
)

$ErrorActionPreference = "Stop"

$Repo = "EaveLuo/filelift"
$Target = "x86_64-pc-windows-msvc"
$Asset = "filelift-$Target.zip"

if ($Version -eq "latest" -and -not [string]::IsNullOrWhiteSpace($env:FILELIFT_VERSION)) {
    $Version = $env:FILELIFT_VERSION
}

if ([string]::IsNullOrWhiteSpace($InstallDir) -and -not [string]::IsNullOrWhiteSpace($env:FILELIFT_INSTALL_DIR)) {
    $InstallDir = $env:FILELIFT_INSTALL_DIR
}

if ([string]::IsNullOrWhiteSpace($InstallDir)) {
    $InstallDir = Join-Path $env:LOCALAPPDATA "Programs\filelift\bin"
}

function Resolve-FileliftTag {
    param([string]$RequestedVersion)

    if ($RequestedVersion -eq "latest") {
        $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
        return $release.tag_name
    }

    if ($RequestedVersion.StartsWith("v")) {
        return $RequestedVersion
    }

    return "v$RequestedVersion"
}

function Add-UserPath {
    param([string]$PathToAdd)

    $userPath = [Environment]::GetEnvironmentVariable("Path", [EnvironmentVariableTarget]::User)
    $entries = @()

    if (-not [string]::IsNullOrWhiteSpace($userPath)) {
        $entries = $userPath -split ";"
    }

    $alreadyPresent = $entries | Where-Object {
        $_.TrimEnd("\") -ieq $PathToAdd.TrimEnd("\")
    }

    if (-not $alreadyPresent) {
        $newPath = if ([string]::IsNullOrWhiteSpace($userPath)) {
            $PathToAdd
        }
        else {
            "$userPath;$PathToAdd"
        }

        [Environment]::SetEnvironmentVariable("Path", $newPath, [EnvironmentVariableTarget]::User)
    }

    $currentEntries = $env:Path -split ";"
    $currentPresent = $currentEntries | Where-Object {
        $_.TrimEnd("\") -ieq $PathToAdd.TrimEnd("\")
    }

    if (-not $currentPresent) {
        $env:Path = "$env:Path;$PathToAdd"
    }
}

$Tag = Resolve-FileliftTag -RequestedVersion $Version
$Url = "https://github.com/$Repo/releases/download/$Tag/$Asset"
$TempDir = Join-Path ([IO.Path]::GetTempPath()) "filelift-$([Guid]::NewGuid())"
$ArchivePath = Join-Path $TempDir $Asset

try {
    New-Item -ItemType Directory -Path $TempDir | Out-Null

    Write-Host "Installing or updating filelift $Tag for $Target"
    Invoke-WebRequest -UseBasicParsing -Uri $Url -OutFile $ArchivePath

    Expand-Archive -Path $ArchivePath -DestinationPath $TempDir -Force
    $Binary = Get-ChildItem -Path $TempDir -Recurse -Filter "filelift.exe" | Select-Object -First 1

    if (-not $Binary) {
        throw "Release asset did not contain filelift.exe"
    }

    $InstalledPath = Join-Path $InstallDir "filelift.exe"
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null

    # Windows locks a running .exe against overwrite but still allows renaming it.
    # When `filelift upgrade` updates the binary that is currently running, the
    # plain Copy-Item fails, so fall back to moving the running binary aside and
    # writing the new one in its place. The leftover .old file is removed on the
    # next install once the previous process has exited.
    $BackupPath = "$InstalledPath.old"
    if (Test-Path $BackupPath) {
        Remove-Item -Path $BackupPath -Force -ErrorAction SilentlyContinue
    }

    try {
        Copy-Item -Path $Binary.FullName -Destination $InstalledPath -Force -ErrorAction Stop
    }
    catch {
        if (Test-Path $InstalledPath) {
            Move-Item -Path $InstalledPath -Destination $BackupPath -Force
            Copy-Item -Path $Binary.FullName -Destination $InstalledPath -Force
        }
        else {
            throw
        }
    }

    Add-UserPath -PathToAdd $InstallDir

    Write-Host "Installed to $InstalledPath"

    # Warn if another filelift earlier on PATH (for example a `cargo install`
    # copy in ~\.cargo\bin) will shadow the binary we just installed, so the user
    # is not surprised by an unchanged version.
    $resolved = (Get-Command filelift -ErrorAction SilentlyContinue | Select-Object -First 1).Source
    if ($resolved -and ($resolved -ne $InstalledPath)) {
        Write-Warning "Another filelift is earlier on your PATH and will be used instead of this install:"
        Write-Host "  in use:    $resolved"
        Write-Host "  installed: $InstalledPath"
        if ($resolved -like "*\.cargo\bin\*") {
            Write-Host "  That copy was installed with cargo. Upgrade it with: cargo install filelift --force"
        }
        else {
            Write-Host "  Remove it or reorder PATH so $InstallDir comes first."
        }
    }

    & $InstalledPath --version
}
finally {
    if (Test-Path $TempDir) {
        Remove-Item -Path $TempDir -Recurse -Force
    }
}
