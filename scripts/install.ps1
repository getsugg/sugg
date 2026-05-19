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

# ==========================================
# Configuration (Modify for your repository)
# ==========================================
$GithubRepo = "axuj/sugg" # TODO: Change to your actual GitHub repo
$AssetName  = "sugg-x86_64-pc-windows-msvc.zip" # TODO: Ensure this matches your release filename

# Installation paths (Aligned with your deploy.rs logic)
$InstallDir = "$env:APPDATA\sugg"
$BinDir     = "$InstallDir\bin"

Write-Host "🚀 Starting Sugg installation..." -ForegroundColor Cyan

# 1. Fetch latest Release info
Write-Host "🔍 Connecting to GitHub to fetch version info..."
$ReleaseApiUrl = "https://api.github.com/repos/$GithubRepo/releases/latest"

try {
    # Force TLS 1.2 for security and compatibility
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    $ReleaseInfo = Invoke-RestMethod -Uri $ReleaseApiUrl -UseBasicParsing
    $Version = $ReleaseInfo.tag_name
    Write-Host "✨ Found latest version: $Version"
} catch {
    Write-Host "❌ Failed to fetch version info. Please check your internet connection or repository name ($GithubRepo)." -ForegroundColor Red
    exit 1
}

# 2. Parse download URL
$DownloadUrl = ($ReleaseInfo.assets | Where-Object { $_.name -eq $AssetName }).browser_download_url
if (-not $DownloadUrl) {
    Write-Host "❌ Could not find asset named $AssetName in release $Version." -ForegroundColor Red
    exit 1
}

# 3. Download and Extract
$TempZip = "$env:TEMP\sugg-$Version.zip"
$TempExtractPath = "$env:TEMP\sugg-extract-$Version"

Write-Host "⬇️  Downloading binaries..."
Invoke-WebRequest -Uri $DownloadUrl -OutFile $TempZip -UseBasicParsing

Write-Host "📦 Extracting and installing to $InstallDir..."
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
    Write-Host "❌ Missing sugg.exe or sugg-engine.exe in the downloaded archive!" -ForegroundColor Red
    Remove-Item -Path $TempExtractPath -Recurse -Force
    Remove-Item -Path $TempZip -Force
    exit 1
}

# Cleanup temporary files
Remove-Item -Path $TempExtractPath -Recurse -Force
Remove-Item -Path $TempZip -Force

# 5. Configure PATH environment variable
Write-Host "⚙️  Configuring environment variables..."
$UserPath = [Environment]::GetEnvironmentVariable('Path', 'User')

if ($UserPath -split ';' -notcontains $BinDir) {
    $NewPath = "$UserPath;$BinDir"
    [Environment]::SetEnvironmentVariable('Path', $NewPath, 'User')
    Write-Host "✅ Added $BinDir to User PATH." -ForegroundColor Green
    Write-Host "⚠️  Note: Please restart your terminal or open a new window for PATH changes to take effect." -ForegroundColor Yellow
} else {
    Write-Host "✅ $BinDir is already in PATH." -ForegroundColor Green
}

Write-Host ""
Write-Host "🎉 Sugg ($Version) installed successfully!" -ForegroundColor Green
Write-Host "   sugg          -> $BinDir\sugg.exe"
Write-Host "   sugg-engine   -> $InstallDir\sugg-engine.exe"
Write-Host ""
Write-Host "Please restart your terminal and type 'sugg' to get started." -ForegroundColor Cyan
