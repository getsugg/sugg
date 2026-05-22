<#
.SYNOPSIS
Sugg Windows Installation Script

.DESCRIPTION
Downloads the latest version of Sugg and installs it to %APPDATA%\sugg.
It automatically adds %APPDATA%\sugg\bin to the User's PATH environment variable.

.EXAMPLE
Invoke-RestMethod -Uri "https://raw.githubusercontent.com/YOUR_GITHUB_NAME/sugg/main/install.ps1" | Invoke-Expression
#>

$ErrorActionPreference = "Stop"

# 检测终端是否支持富文本 Emoji
$SupportsEmoji = ($env:WT_SESSION -or ($env:TERM_PROGRAM -eq "vscode") -or ($PSVersionTable.PSVersion.Major -ge 6)) -and (-not $env:NO_COLOR)

function Write-Step {
    param([string]$Rich, [string]$Fallback, [string]$Text, [string]$Color)
    $Icon = if ($SupportsEmoji) { $Rich } else { $Fallback }
    if ($Color) {
        Write-Host "$Icon $Text" -ForegroundColor $Color
    } else {
        Write-Host "$Icon $Text"
    }
}

# ==========================================
# Configuration (Modify for your repository)
# ==========================================
$GithubRepo = "axuj/sugg" # TODO: Change to your actual GitHub repo
$AssetName  = "sugg-x86_64-pc-windows-msvc.zip" # TODO: Ensure this matches your release filename

# Installation paths (Aligned with your deploy.rs logic)
$InstallDir = "$env:APPDATA\sugg"
$BinDir     = "$InstallDir\bin"

Write-Step "🚀" ">" "Starting Sugg installation..." "Cyan"

# 1. Fetch latest Release info
Write-Step "🔍" "»" "Connecting to GitHub to fetch version info..."
$ReleaseApiUrl = "https://api.github.com/repos/$GithubRepo/releases/latest"

try {
    # Force TLS 1.2 for security and compatibility
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    $ReleaseInfo = Invoke-RestMethod -Uri $ReleaseApiUrl -UseBasicParsing
    $Version = $ReleaseInfo.tag_name
    Write-Step "🏷️" "@" "Found latest version: $Version"
} catch {
    Write-Step "❌" "×" "Failed to fetch version info. Please check your internet connection or repository name ($GithubRepo)." "Red"
    exit 1
}

# 2. Parse download URL
$DownloadUrl = ($ReleaseInfo.assets | Where-Object { $_.name -eq $AssetName }).browser_download_url
if (-not $DownloadUrl) {
    Write-Step "❌" "×" "Could not find asset named $AssetName in release $Version." "Red"
    exit 1
}

# 3. Download and Extract
$TempZip = "$env:TEMP\sugg-$Version.zip"
$TempExtractPath = "$env:TEMP\sugg-extract-$Version"

Write-Step "📥" "↓" "Downloading binaries..."
Invoke-WebRequest -Uri $DownloadUrl -OutFile $TempZip -UseBasicParsing

Write-Step "📦" "o" "Extracting and installing to $InstallDir..."
if (-not (Test-Path $InstallDir)) { New-Item -ItemType Directory -Path $InstallDir | Out-Null }
if (-not (Test-Path $BinDir))     { New-Item -ItemType Directory -Path $BinDir | Out-Null }

# Clean up previous temp extraction if it exists
if (Test-Path $TempExtractPath) { Remove-Item -Path $TempExtractPath -Recurse -Force }
Expand-Archive -Path $TempZip -DestinationPath $TempExtractPath -Force

# 4. Deploy files according to Sugg architecture
# Search recursively for executables in case of nested folders in the zip
$ExtractedSugg = Get-ChildItem -Path $TempExtractPath -Recurse -Filter "sugg.exe" | Select-Object -First 1
$ExtractedEngine = Get-ChildItem -Path $TempExtractPath -Recurse -Filter "sugg-engine.exe" | Select-Object -First 1

if ($ExtractedSugg -and $ExtractedEngine) {
    Move-Item -Path $ExtractedSugg.FullName -Destination "$BinDir\sugg.exe" -Force
    Move-Item -Path $ExtractedEngine.FullName -Destination "$InstallDir\sugg-engine.exe" -Force
} else {
    Write-Step "❌" "×" "Missing sugg.exe or sugg-engine.exe in the downloaded archive!" "Red"
    Remove-Item -Path $TempExtractPath -Recurse -Force
    Remove-Item -Path $TempZip -Force
    exit 1
}

# Cleanup temporary files
Remove-Item -Path $TempExtractPath -Recurse -Force
Remove-Item -Path $TempZip -Force

# 5. Configure PATH environment variable
Write-Step "🛠️" "*" "Configuring environment variables..."
$UserPath = [Environment]::GetEnvironmentVariable('Path', 'User')

if ($UserPath -split ';' -notcontains $BinDir) {
    $NewPath = "$UserPath;$BinDir"
    [Environment]::SetEnvironmentVariable('Path', $NewPath, 'User')
    Write-Step "✅" "√" "Added $BinDir to User PATH." "Green"
    Write-Step "❗" "!" "Note: Please restart your terminal or open a new window for PATH changes to take effect." "Yellow"
} else {
    Write-Step "✅" "√" "$BinDir is already in PATH." "Green"
}

Write-Host ""
Write-Step "🎉" "*" "Sugg ($Version) installed successfully!" "Green"
Write-Host "   sugg          -> $BinDir\sugg.exe"
Write-Host "   sugg-engine   -> $InstallDir\sugg-engine.exe"
Write-Host ""
Write-Host "Please restart your terminal and type 'sugg' to get started." -ForegroundColor Cyan
