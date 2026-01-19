<#
.SYNOPSIS
    mylm Windows Binary Installer (Consumer)
.DESCRIPTION
    Downloads and installs pre-compiled mylm binaries from GitHub.
    Fast installation with no build-time dependencies.
.NOTES
    This script is for users who want to use mylm without compiling from source.
#>

param(
    [string]$InstallPrefix,
    [string]$Repo = "edward/mylm", # Adjust to actual repository
    [switch]$SkipVerify
)

# Set default install prefix
if (-not $InstallPrefix) {
    $InstallPrefix = "$env:LOCALAPPDATA\mylm"
}

$ErrorActionPreference = "Stop"

# Configuration
$BinaryDir = "$InstallPrefix\bin"
$BinaryDest = "$BinaryDir\mylm.exe"
$TempDir = "$env:TEMP\mylm-install"

# Colors
$Colors = @{ Red = "Red"; Green = "Green"; Yellow = "Yellow"; Cyan = "Cyan"; Magenta = "Magenta"; White = "White" }
function Write-ColorOutput { param([string]$Message, [string]$Color = "White") Write-Host $Message -ForegroundColor $Colors[$Color] }

function Get-LatestRelease {
    Write-ColorOutput "🔍 Checking for latest release of $Repo..." Cyan
    try {
        $url = "https://api.github.com/repos/$Repo/releases/latest"
        $release = Invoke-RestMethod -Uri $url -UseBasicParsing
        return $release
    }
    catch {
        Write-ColorOutput "❌ Failed to fetch latest release: $_" Red
        exit 1
    }
}

function Install-Binary {
    param($Release)
    
    $tag = $Release.tag_name
    Write-ColorOutput "📦 Found release $tag" Green
    
    # Find Windows x86_64 asset
    $asset = $Release.assets | Where-Object { $_.name -like "*windows-msvc.zip" }
    if (-not $asset) {
        Write-ColorOutput "❌ Could not find Windows binary in release $tag" Red
        exit 1
    }
    
    if (-not (Test-Path $TempDir)) { New-Item -ItemType Directory -Path $TempDir -Force | Out-Null }
    
    $zipPath = "$TempDir\mylm.zip"
    Write-ColorOutput "📥 Downloading $($asset.name)..." Cyan
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zipPath -UseBasicParsing
    
    # Verification
    if (-not $SkipVerify) {
        $checksumAsset = $Release.assets | Where-Object { $_.name -eq "SHA256SUMS" }
        if ($checksumAsset) {
            Write-ColorOutput "🔍 Verifying checksum..." Cyan
            $sumsPath = "$TempDir\SHA256SUMS"
            Invoke-WebRequest -Uri $checksumAsset.browser_download_url -OutFile $sumsPath -UseBasicParsing
            
            $fileHash = (Get-FileHash -Path $zipPath -Algorithm SHA256).Hash.ToLower()
            $expectedHashLine = Get-Content $sumsPath | Where-Object { $_ -match [regex]::Escape($asset.name) }
            
            if ($expectedHashLine -match "^([a-f0-9]{64})") {
                $expectedHash = $matches[1].ToLower()
                if ($fileHash -eq $expectedHash) {
                    Write-ColorOutput "✅ Checksum verified!" Green
                } else {
                    Write-ColorOutput "❌ Checksum mismatch! The file may be corrupted or tampered with." Red
                    Write-ColorOutput "   Expected: $expectedHash" Yellow
                    Write-ColorOutput "   Actual:   $fileHash" Yellow
                    exit 1
                }
            } else {
                Write-ColorOutput "⚠️  Could not find hash for $($asset.name) in SHA256SUMS. Skipping verification." Yellow
            }
        } else {
            Write-ColorOutput "⚠️  No SHA256SUMS found in release. Skipping verification." Yellow
        }
    }
    
    # Extraction
    Write-ColorOutput "📦 Extracting..." Cyan
    if (-not (Test-Path $BinaryDir)) { New-Item -ItemType Directory -Path $BinaryDir -Force | Out-Null }
    
    # Check if busy
    if (Test-Path $BinaryDest) {
        try {
            $file = [System.IO.File]::Open($BinaryDest, 'Open', 'ReadWrite', 'None')
            $file.Close()
        } catch {
            Write-ColorOutput "⚠️  $BinaryDest is in use. Please close any running mylm instances." Yellow
            Read-Host "Press Enter after closing the program..."
        }
    }
    
    Expand-Archive -Path $zipPath -DestinationPath $TempDir -Force
    $extractedExe = Get-ChildItem -Path $TempDir -Filter "mylm.exe" -Recurse | Select-Object -First 1
    
    if ($extractedExe) {
        Copy-Item -Path $extractedExe.FullName -Destination $BinaryDest -Force
        Write-ColorOutput "✅ Installed mylm to $BinaryDest" Green
    } else {
        Write-ColorOutput "❌ Could not find mylm.exe in the downloaded archive." Red
        exit 1
    }
    
    # Cleanup
    Remove-Item -Path $TempDir -Recurse -Force -ErrorAction SilentlyContinue
}

function Configure-Environment {
    Write-ColorOutput "🔍 Configuring environment..." Cyan
    
    # Add to PATH
    $userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
    if ($userPath -notlike "*$BinaryDir*") {
        Write-ColorOutput "➕ Adding $BinaryDir to user PATH..." Green
        [Environment]::SetEnvironmentVariable("PATH", "$userPath;$BinaryDir", "User")
        $env:PATH = "$env:PATH;$BinaryDir"
    }
    
    # Create Wrapper/Alias
    $profilePath = $PROFILE.CurrentUserAllHosts
    $profileDir = Split-Path $profilePath -Parent
    if (-not (Test-Path $profileDir)) { New-Item -ItemType Directory -Path $profileDir -Force | Out-Null }
    if (-not (Test-Path $profilePath)) { New-Item -ItemType File -Path $profilePath -Force | Out-Null }
    
    $aliasFunc = @"

# --- mylm alias ---
function ai {
    param([Parameter(ValueFromRemainingArguments=`$true)]`$Arguments)
    & "$BinaryDest" @Arguments
}
# --- end mylm alias ---
"@
    
    if (-not (Select-String -Path $profilePath -Pattern "function ai {" -Quiet)) {
        Add-Content -Path $profilePath -Value $aliasFunc
        Write-ColorOutput "✅ Added 'ai' alias to PowerShell profile." Green
    }
}

# Main
Write-ColorOutput "🔷 mylm Binary Installer" Magenta
Write-ColorOutput "========================" Magenta

$release = Get-LatestRelease
Install-Binary -Release $release
Configure-Environment

Write-Host ""
Write-ColorOutput "🎉 Installation successful!" Green
Write-ColorOutput "💡 Restart your terminal or run '. `$PROFILE' to start using 'ai'." Cyan
Write-ColorOutput "🚀 Run 'ai setup' to configure your AI providers." Cyan
