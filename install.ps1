<#
.SYNOPSIS
    mylm Windows Installation Script
.DESCRIPTION
    Native Windows installer for mylm AI assistant with PowerShell integration
.NOTES
    This script provides a Windows-native installation alternative to WSL2
    
    USAGE:
    - PowerShell 7+:  pwsh -ExecutionPolicy Bypass -File install.ps1
    - Windows PowerShell: powershell -ExecutionPolicy Bypass -File install.ps1
#>

# Detect if we're running in cmd.exe (shebang failed)
$inCmd = $null -ne $env:COMSPEC -and $env:COMSPEC.EndsWith("cmd.exe")
if ($inCmd -and -not $MyInvocation.MyCommand.Path) {
    Write-Host "❌ Error: 'pwsh' is not recognized." -ForegroundColor Red
    Write-Host ""
    Write-Host "To run this script, use one of these commands:" -ForegroundColor Yellow
    Write-Host ""
    Write-Host "  Option 1 - PowerShell 7+ (recommended):" -ForegroundColor Cyan
    Write-Host "    pwsh -ExecutionPolicy Bypass -File install.ps1" -ForegroundColor Gray
    Write-Host ""
    Write-Host "  Option 2 - Windows PowerShell (built-in):" -ForegroundColor Cyan
    Write-Host "    powershell -ExecutionPolicy Bypass -File install.ps1" -ForegroundColor Gray
    Write-Host ""
    Write-Host "  Option 3 - Install PowerShell 7+:" -ForegroundColor Cyan
    Write-Host "    winget install Microsoft.PowerShell" -ForegroundColor Gray
    Write-Host ""
    exit 1
}

param(
    [switch]$Force,
    [switch]$SkipTmux,
    [string]$InstallPrefix = "$env:LOCALAPPDATA\mylm",
    [string]$BuildProfile = "release"
)

# Exit on error
$ErrorActionPreference = "Stop"

# Configuration
$ConfigDir = "$env:APPDATA\mylm"
$ConfigFile = "$ConfigDir\mylm.yaml"
$BinaryDest = "$InstallPrefix\bin\mylm.exe"
$WindowsWrapperDir = "$InstallPrefix\bin"

# Colors for output
$Colors = @{
    Red = "Red"
    Green = "Green"
    Yellow = "Yellow"
    Cyan = "Cyan"
    Magenta = "Magenta"
    White = "White"
}

function Write-ColorOutput {
    param(
        [string]$Message,
        [string]$Color = "White"
    )
    Write-Host $Message -ForegroundColor $Colors[$Color]
}

function Test-CommandExists {
    param([string]$Command)
    try {
        $null = Get-Command $Command -ErrorAction Stop
        return $true
    }
    catch {
        return $false
    }
}

function Get-CurrentVersion {
    if (Test-Path "Cargo.toml") {
        $content = Get-Content "Cargo.toml" -Raw
        if ($content -match 'version\s*=\s*"([^"]+)"') {
            return $matches[1]
        }
    }
    return "unknown"
}

function Get-InstalledVersion {
    if (Test-Path $BinaryDest) {
        try {
            $versionOutput = & $BinaryDest --version 2>$null
            if ($versionOutput -match '(\d+\.\d+\.\d+)') {
                return $matches[1]
            }
        }
        catch {
            return "none"
        }
    }
    return "none"
}

function Test-BinaryBusy {
    param([string]$TargetPath)
    
    if (-not (Test-Path $TargetPath)) {
        return $false
    }
    
    try {
        # Try to open the file for exclusive access
        $fileStream = [System.IO.File]::Open($TargetPath, 'Open', 'ReadWrite', 'None')
        $fileStream.Close()
        return $false
    }
    catch {
        return $true
    }
}

function Stop-ProcessesUsingBinary {
    param([string]$TargetPath)
    
    Write-ColorOutput "⚠️  Binary $TargetPath is currently in use." Yellow
    $killIt = Read-Host "Kill running processes using it? [y/N]"
    
    if ($killIt -match '^[Yy]$') {
        # Get the process name from the binary
        $processName = [System.IO.Path]::GetFileNameWithoutExtension($TargetPath)
        
        # Find and stop processes
        $processes = Get-Process -Name $processName -ErrorAction SilentlyContinue
        foreach ($process in $processes) {
            try {
                $process.CloseMainWindow() | Out-Null
                Start-Sleep -Milliseconds 500
                if (-not $process.HasExited) {
                    $process.Kill()
                }
                Write-ColorOutput "✅ Stopped process $($process.Id)" Green
            }
            catch {
                Write-ColorOutput "⚠️  Could not stop process $($process.Id): $_" Yellow
            }
        }
        
        # Wait a bit and check again
        Start-Sleep -Seconds 1
        if (Test-BinaryBusy $TargetPath) {
            Write-ColorOutput "❌ Aborting: target file is still busy." Red
            exit 1
        }
    }
    else {
        Write-ColorOutput "❌ Aborting: target file is busy." Red
        exit 1
    }
}

function Install-RustIfNeeded {
    if (-not (Test-CommandExists "cargo")) {
        Write-ColorOutput "❌ Rust/Cargo not found." Red
        $installRust = Read-Host "Would you like to install Rust now? [Y/n]"
        
        if ($installRust -notmatch '^[Nn]$') {
            Write-ColorOutput "🚀 Installing Rust..." Cyan
            
            # Try multiple rustup URLs (CDN might be down)
            $rustupUrls = @(
                "https://win.rustup.rs/x86_64-pc-windows-msvc",
                "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe",
                "https://rustup.rs.rs/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe"
            )
            
            $rustupPath = "$env:TEMP\rustup-init.exe"
            $downloadSuccess = $false
            
            foreach ($rustupUrl in $rustupUrls) {
                try {
                    Write-ColorOutput "Trying: $rustupUrl" Cyan
                    Invoke-WebRequest -Uri $rustupUrl -OutFile $rustupPath -UseBasicParsing -TimeoutSec 30
                    $downloadSuccess = $true
                    Write-ColorOutput "✅ Downloaded rustup successfully!" Green
                    break
                }
                catch {
                    Write-ColorOutput "⚠️  Download failed: $($_.Exception.Message)" Yellow
                }
            }
            
            if (-not $downloadSuccess) {
                Write-ColorOutput "❌ All rustup download sources failed." Red
                Write-Host ""
                Write-ColorOutput "Manual installation options:" Yellow
                Write-Host "  1. Download from: https://rustup.rs/" -ForegroundColor Gray
                Write-Host "  2. Or install via chocolatey: choco install rustup" -ForegroundColor Gray
                Write-Host "  3. Or install via winget: winget install Rustlang.Rustup" -ForegroundColor Gray
                Write-Host ""
                
                $manualInstall = Read-Host "Install manually and then restart this script? [y/N]"
                if ($manualInstall -match '^[Yy]$') {
                    Write-ColorOutput "💡 After installing Rust, restart your terminal and run this script again." Cyan
                }
                exit 1
            }
            
            try {
                Write-ColorOutput "Running rustup installer..." Cyan
                & $rustupPath -y --default-toolchain stable
                
                # Add cargo to PATH for current session
                $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
                
                Write-ColorOutput "✅ Rust installed." Green
                Write-ColorOutput "⚠️  IMPORTANT: You MUST restart your terminal or run 'refreshenv' before continuing if you are in a new shell." Yellow
                Write-ColorOutput "💡 This script will continue now using the updated PATH." Cyan
            }
            catch {
                Write-ColorOutput "❌ Failed to run rustup installer: $_" Red
                Write-ColorOutput "   The installer may have failed. Please check and try again." Yellow
                exit 1
            }
            finally {
                if (Test-Path $rustupPath) {
                    Remove-Item $rustupPath -Force
                }
            }
        }
        else {
            Write-ColorOutput "❌ Error: Rust is required to build mylm. Exiting." Red
            exit 1
        }
    }
    
    # Verify cargo is now available
    if (-not (Test-CommandExists "cargo")) {
        Write-ColorOutput "❌ Cargo not found in PATH. Please restart your terminal and try again." Red
        Write-Host ""
        Write-ColorOutput "Manual steps:" Yellow
        Write-Host "  1. Close this terminal" -ForegroundColor Gray
        Write-Host "  2. Open a new terminal (cargo should be in PATH)" -ForegroundColor Gray
        Write-Host "  3. Run this script again" -ForegroundColor Gray
        exit 1
    }
}

function Install-SystemDependencies {
    Write-ColorOutput "🔍 Checking for system dependencies..." Cyan
    
    $missingDeps = @()
    
    # Check for protoc
    if (-not (Test-CommandExists "protoc")) {
        Write-ColorOutput "⚠️  protoc (Protocol Buffers compiler) not found." Yellow
        $missingDeps += "protoc"
    }
    
    # Check for Visual Studio Build Tools
    $vsWherePath = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    $vsInstalled = $false
    
    if (Test-Path $vsWherePath) {
        $vsWhereOutput = & $vsWherePath -latest -requires Microsoft.VisualStudio.Workload.NativeDesktop -property installationPath
        if ($vsWhereOutput) {
            $vsInstalled = $true
            Write-ColorOutput "✅ Visual Studio Build Tools found." Green
        }
    }
    
    if (-not $vsInstalled) {
        Write-ColorOutput "⚠️  Visual Studio Build Tools not found." Yellow
        $missingDeps += "visual-studio-build-tools"
    }
    
    # Check for sccache
    if (-not (Test-CommandExists "sccache")) {
        Write-ColorOutput "⚠️  sccache is not installed, but it is recommended for faster builds." Yellow
        $installSccache = Read-Host "Would you like to install sccache now? [Y/n]"
        if ($installSccache -notmatch '^[Nn]$') {
            Write-ColorOutput "🚀 Installing sccache via cargo..." Cyan
            cargo install sccache
            if ($LASTEXITCODE -eq 0) {
                Write-ColorOutput "✅ sccache installed." Green
            }
            else {
                Write-ColorOutput "⚠️  Failed to install sccache via cargo. Build will continue without it." Yellow
            }
        }
    }
    
    if ($missingDeps.Count -gt 0) {
        Write-ColorOutput "⚠️  Missing system dependencies: $($missingDeps -join ', ')" Yellow
        Write-ColorOutput "🔒 This installer will continue, but you may need to install these manually." Cyan
        Write-ColorOutput ""
        Write-ColorOutput "➡️  For protoc:" Cyan
        Write-ColorOutput "   Option 1: choco install protoc" Cyan
        Write-ColorOutput "   Option 2: Download from https://github.com/protocolbuffers/protobuf/releases" Cyan
        Write-ColorOutput ""
        Write-ColorOutput "➡️  For Visual Studio Build Tools:" Cyan
        Write-ColorOutput "   Download: https://visualstudio.microsoft.com/downloads/#build-tools-for-visual-studio-2022" Cyan
        Write-ColorOutput "   Required workload: 'Desktop development with C++'" Cyan
        
        $continue = Read-Host "Continue anyway? [y/N]"
        if ($continue -notmatch '^[Yy]$') {
            exit 1
        }
    }
}

function Build-Binary {
    param(
        [bool]$ForceRebuild,
        [string]$InitialProfile
    )
    
    # Determine build profile
    $profile = $InitialProfile
    if (-not $profile) {
        if ((Test-Path "target\release\mylm.exe") -and (Test-Path "target\debug\mylm.exe")) {
            $releaseTime = (Get-Item "target\release\mylm.exe").LastWriteTime
            $debugTime = (Get-Item "target\debug\mylm.exe").LastWriteTime
            if ($releaseTime -gt $debugTime) {
                $profile = "release"
            }
            else {
                $profile = "debug"
            }
        }
        elseif (Test-Path "target\release\mylm.exe") {
            $profile = "release"
        }
        elseif (Test-Path "target\debug\mylm.exe") {
            $profile = "debug"
        }
    }
    
    if (-not $profile) {
        $buildType = Read-Host "Use optimized release build (20 min) or fast dev build (7 min)? [r/D]"
        if ($buildType -match '^[Rr]$') {
            $profile = "release"
        }
        else {
            $profile = "debug"
        }
    }
    
    $script:BuildProfile = $profile
    $binaryPath = "target\$profile\mylm.exe"
    
    if ((-not $ForceRebuild) -and (Test-Path $binaryPath)) {
        Write-ColorOutput "✨ Found an existing $profile binary at $binaryPath" Green
        $rebuild = Read-Host "Would you like to rebuild it to ensure it's the latest version? [y/N]"
        if ($rebuild -notmatch '^[Yy]$') {
            Write-ColorOutput "⏭️  Skipping build, using existing binary." Cyan
            return
        }
    }
    
    Write-ColorOutput "🚀 Building mylm in $profile mode..." Cyan
    
    # Set up build environment for Windows
    $env:RUSTFLAGS = "-C target-feature=+crt-static"
    
    # Set build target
    $env:CARGO_BUILD_TARGET = "x86_64-pc-windows-msvc"
    
    if ($profile -eq "release") {
        if (Test-CommandExists "sccache") {
            $env:RUSTC_WRAPPER = "sccache"
            Write-ColorOutput "Building with sccache..." Cyan
            cargo build --release
        }
        else {
            cargo build --release
        }
    }
    else {
        if (Test-CommandExists "sccache") {
            $env:RUSTC_WRAPPER = "sccache"
            Write-ColorOutput "Building with sccache..." Cyan
            cargo build
        }
        else {
            cargo build
        }
    }
    
    if ($LASTEXITCODE -ne 0) {
        Write-ColorOutput "❌ Build failed." Red
        Write-ColorOutput ""
        Write-ColorOutput "Troubleshooting steps:" Yellow
        Write-ColorOutput "1. Ensure Visual Studio Build Tools are installed" Cyan
        Write-ColorOutput "2. Run this script from 'Developer PowerShell for VS'" Cyan
        Write-ColorOutput "3. Try running 'cargo clean' and rebuilding" Cyan
        exit 1
    }
    
    Write-ColorOutput "✅ Build completed successfully." Green
}

function Install-Binary {
    param([string]$Profile)
    
    if (-not $Profile) {
        if ((Test-Path "target\release\mylm.exe") -and (Test-Path "target\debug\mylm.exe")) {
            $releaseTime = (Get-Item "target\release\mylm.exe").LastWriteTime
            $debugTime = (Get-Item "target\debug\mylm.exe").LastWriteTime
            if ($releaseTime -gt $debugTime) {
                $Profile = "release"
            }
            else {
                $Profile = "debug"
            }
        }
        elseif (Test-Path "target\release\mylm.exe") {
            $Profile = "release"
        }
        elseif (Test-Path "target\debug\mylm.exe") {
            $Profile = "debug"
        }
    }
    
    $sourcePath = "target\$Profile\mylm.exe"
    if (-not (Test-Path $sourcePath)) {
        Write-ColorOutput "❌ Error: Could not find binary at $sourcePath" Red
        Write-ColorOutput "   Please ensure the build completed successfully." Red
        exit 1
    }
    
    Write-ColorOutput "📦 Installing binary from $sourcePath to $BinaryDest..." Cyan
    
    # Create destination directory
    $destDir = Split-Path $BinaryDest -Parent
    if (-not (Test-Path $destDir)) {
        New-Item -ItemType Directory -Path $destDir -Force | Out-Null
    }
    
    # Check if binary is busy
    if (Test-BinaryBusy $BinaryDest) {
        Stop-ProcessesUsingBinary $BinaryDest
    }
    
    # Copy binary
    Copy-Item -Path $sourcePath -Destination $BinaryDest -Force
    
    Write-ColorOutput "✅ Binary installed successfully." Green
}

function Add-ToPath {
    Write-ColorOutput "🔍 Ensuring $WindowsWrapperDir is on your PATH..." Cyan
    
    $userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
    $pathEntries = $userPath -split ';'
    
    # Normalize paths for comparison
    $normalizedEntries = $pathEntries | ForEach-Object { $_.TrimEnd('\') }
    $normalizedWrapperDir = $WindowsWrapperDir.TrimEnd('\')
    
    if ($normalizedEntries -notcontains $normalizedWrapperDir) {
        $newPath = "$userPath;$WindowsWrapperDir"
        [Environment]::SetEnvironmentVariable("PATH", $newPath, "User")
        
        # Also update current session
        $env:PATH = "$env:PATH;$WindowsWrapperDir"
        
        Write-ColorOutput "✅ Added $WindowsWrapperDir to your user PATH." Green
        Write-ColorOutput "💡 Restart your terminal for PATH changes to take effect." Cyan
    }
    else {
        Write-ColorOutput "✅ $WindowsWrapperDir is already in your PATH." Green
    }
}

function Set-PowerShellAlias {
    param([bool]$Mandatory)
    
    Write-ColorOutput "🔍 Configuring PowerShell alias..." Cyan
    
    $profilePath = $PROFILE.CurrentUserAllHosts
    $profileDir = Split-Path $profilePath -Parent
    
    if (-not (Test-Path $profileDir)) {
        New-Item -ItemType Directory -Path $profileDir -Force | Out-Null
    }
    
    if (-not (Test-Path $profilePath)) {
        New-Item -ItemType File -Path $profilePath -Force | Out-Null
    }
    
    $chosenAlias = Read-Host "Set your preferred alias to call mylm [default: ai]"
    if (-not $chosenAlias) {
        $chosenAlias = "ai"
    }
    
    # Basic validation: no spaces
    if ($chosenAlias -match '\s') {
        Write-ColorOutput "❌ Alias cannot contain spaces. Falling back to 'ai'." Red
        $chosenAlias = "ai"
    }
    
    # Check for conflicts with existing commands
    $existingCommand = $null
    try {
        $existingCommand = Get-Command $chosenAlias -ErrorAction Stop
    }
    catch {
        # Command doesn't exist, which is fine
    }
    
    if ($existingCommand -and -not (Select-String -Path $profilePath -Pattern "function $chosenAlias" -Quiet)) {
        Write-ColorOutput "⚠️  Warning: '$chosenAlias' already exists as a command: $($existingCommand.Source)" Yellow
        $confirmConflict = Read-Host "Are you sure you want to use '$chosenAlias'? [y/N]"
        if ($confirmConflict -notmatch '^[Yy]$') {
            Write-ColorOutput "⏭️  Skipping alias setup." Cyan
            return
        }
    }
    
    # Remove existing alias if it exists
    $content = Get-Content $profilePath -Raw
    if ($content -and $content -match "function $chosenAlias") {
        Write-ColorOutput "⚠️  Found an existing '$chosenAlias' in $profilePath" Yellow
        if ($Mandatory) {
            $content = $content -replace "(?s)function $chosenAlias\s*{[^}]*}\s*", ""
            Set-Content -Path $profilePath -Value $content.Trim()
            Write-ColorOutput "✅ Removed existing alias." Green
        }
        else {
            $replaceAlias = Read-Host "Would you like to replace it? [y/N]"
            if ($replaceAlias -match '^[Yy]$') {
                $content = $content -replace "(?s)function $chosenAlias\s*{[^}]*}\s*", ""
                Set-Content -Path $profilePath -Value $content.Trim()
                Write-ColorOutput "✅ Removed existing alias." Green
            }
            else {
                Write-ColorOutput "⏭️  Skipping alias setup." Cyan
                return
            }
        }
    }
    
    # Add new alias using function syntax (more PowerShell-native)
    $aliasFunction = @"

# --- mylm alias ---
function $chosenAlias {
    param([Parameter(ValueFromRemainingArguments=`$true)]`$Arguments)
    & "$BinaryDest" @Arguments
}
# --- end mylm alias ---
"@
    
    Add-Content -Path $profilePath -Value $aliasFunction
    Write-ColorOutput "✅ Alias '$chosenAlias' added to $profilePath" Green
    Write-ColorOutput "💡 Restart PowerShell or run `. `$profilePath` to apply changes." Cyan
}

function Set-TmuxAutoStart {
    Write-ColorOutput "🔍 Configuring Seamless Terminal Context..." Cyan
    Write-ColorOutput "💡 Note: tmux is not available on Windows." Yellow
    Write-ColorOutput "   This feature provides terminal context capture on Linux/macOS." Yellow
    Write-ColorOutput "   On Windows, consider using Windows Terminal with multiple panes." Yellow
    
    if (-not $SkipTmux) {
        $enableTmux = Read-Host "Would you like to see this message on future runs? [y/N]"
        if ($enableTmux -match '^[Yy]$') {
            Write-ColorOutput "✅ Configuration noted. You can use Windows Terminal for similar functionality." Green
        }
    }
}

function Create-WindowsWrappers {
    Write-ColorOutput "🔍 Creating Windows wrapper scripts..." Cyan
    
    # Create ai.cmd batch wrapper
    $cmdWrapperPath = "$InstallPrefix\bin\ai.cmd"
    $cmdContent = "@echo off
`"$BinaryDest`" %*
"
    Set-Content -Path $cmdWrapperPath -Value $cmdContent -Encoding ASCII
    Write-ColorOutput "✅ Created $cmdWrapperPath" Green
    
    # Create ai.ps1 PowerShell wrapper
    $ps1WrapperPath = "$InstallPrefix\bin\ai.ps1"
    $ps1Content = "#!/usr/bin/env pwsh
param([Parameter(ValueFromRemainingArguments=`$true)]`$Arguments)
& `"$BinaryDest`" @Arguments
"
    Set-Content -Path $ps1WrapperPath -Value $ps1Content -Encoding UTF8
    Write-ColorOutput "✅ Created $ps1WrapperPath" Green
}

function Start-SetupWizard {
    param([bool]$Mandatory)
    
    Write-ColorOutput "⚙️  Running Configuration Setup..." Cyan
    Write-ColorOutput "💡 Note: If the configuration fails here, simply move on." Cyan
    Write-ColorOutput "   You can always configure your providers later by running 'ai' or 'mylm'." Cyan
    
    if ($Mandatory) {
        & $BinaryDest setup
    }
    else {
        $launchSetup = Read-Host "Would you like to run the configuration wizard (setup)? [y/N]"
        if ($launchSetup -match '^[Yy]$') {
            & $BinaryDest setup
        }
    }
}

function Show-Menu {
    $current = Get-CurrentVersion
    $installed = Get-InstalledVersion
    
    Write-Host "------------------------------------------------" -ForegroundColor Cyan
    Write-Host "   🤖 mylm Windows Installation Wizard v$current   " -ForegroundColor Cyan
    Write-Host "------------------------------------------------" -ForegroundColor Cyan
    Write-Host "Status: Installed v$installed" -ForegroundColor Green
    Write-Host "------------------------------------------------" -ForegroundColor Cyan
    Write-Host "1) 🚀 Fresh Installation (Full Wipe & Setup)" -ForegroundColor White
    Write-Host "2) 🔄 Update Existing (Build & Update Binary Only)" -ForegroundColor White
    Write-Host "3) 🔗 Setup PowerShell Alias Only" -ForegroundColor White
    Write-Host "4) ⚙️  Run Configuration Wizard (setup)" -ForegroundColor White
    Write-Host "5) ❌ Exit" -ForegroundColor White
    Write-Host "------------------------------------------------" -ForegroundColor Cyan
}

function Start-FreshInstallation {
    Write-ColorOutput "🌟 Starting Fresh Installation..." Magenta
    
    Install-RustIfNeeded
    Install-SystemDependencies
    
    # Clean previous build if requested
    $doClean = Read-Host "Would you like to clean previous build artifacts? (Forces full rebuild) [y/N]"
    if ($doClean -match '^[Yy]$') {
        Write-ColorOutput "🧹 Cleaning previous build artifacts..." Cyan
        cargo clean
    }
    
    Build-Binary -ForceRebuild $true -InitialProfile $BuildProfile
    Install-Binary -Profile $BuildProfile
    Add-ToPath
    Create-WindowsWrappers
    Set-PowerShellAlias -Mandatory $true
    Set-TmuxAutoStart
    Start-SetupWizard -Mandatory $true
    
    Write-Host ""
    Write-ColorOutput "✅ Fresh installation complete!" Green
    Write-ColorOutput "   Binary: $BinaryDest" Cyan
    Write-ColorOutput "   Config: $ConfigDir\mylm.yaml" Cyan
    Write-Host ""
    Write-ColorOutput "💡 Next steps:" Yellow
    Write-ColorOutput "   1. Restart PowerShell or run 'refreshenv'" Cyan
    Write-ColorOutput "   2. Run 'ai' to start using mylm" Cyan
    Write-ColorOutput "   3. Or run '. \$PROFILE' to apply alias immediately" Cyan
}

function Start-UpdateExisting {
    Write-ColorOutput "🔄 Checking for updates..." Cyan
    
    $current = Get-CurrentVersion
    $installed = Get-InstalledVersion
    
    Write-ColorOutput "📦 Local Source Version: $current" Cyan
    Write-ColorOutput "📦 Installed Binary Version: $installed" Cyan
    
    if ($current -eq $installed) {
        Write-ColorOutput "✨ You already have the latest version installed ($installed)." Green
        $forceUpdate = Read-Host "Force rebuild and reinstall anyway? [y/N]"
        if ($forceUpdate -notmatch '^[Yy]$') {
            return
        }
    }
    else {
        Write-ColorOutput "🆕 A different version is available. Updating..." Cyan
    }
    
    Install-RustIfNeeded
    Install-SystemDependencies
    
    Build-Binary -ForceRebuild $false -InitialProfile $BuildProfile
    Install-Binary -Profile $BuildProfile
    Add-ToPath
    
    Write-Host ""
    Write-ColorOutput "✅ Update complete! (Your configuration and aliases were preserved)" Green
}

# Check for PowerShell 7 and add to PATH if needed
function Add-PowerShell7ToPath {
    $ps7Paths = @(
        "$env:ProgramFiles\PowerShell\7\pwsh.exe",
        "${env:ProgramFiles(x86)}\PowerShell\7\pwsh.exe",
        "$env:LOCALAPPDATA\Microsoft\PowerShell\7\pwsh.exe"
    )
    
    foreach ($ps7Path in $ps7Paths) {
        if (Test-Path $ps7Path) {
            $ps7Dir = Split-Path $ps7Path -Parent
            $userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
            $pathEntries = $userPath -split ';'
            $normalizedEntries = $pathEntries | ForEach-Object { $_.TrimEnd('\') }
            $normalizedPs7Dir = $ps7Dir.TrimEnd('\')
            
            if ($normalizedEntries -notcontains $normalizedPs7Dir) {
                Write-ColorOutput "🔍 Found PowerShell 7 at: $ps7Path" Cyan
                Write-ColorOutput "   Adding to PATH..." Cyan
                $newPath = "$userPath;$ps7Dir"
                [Environment]::SetEnvironmentVariable("PATH", $newPath, "User")
                $env:PATH = "$env:PATH;$ps7Dir"
                Write-ColorOutput "✅ Added PowerShell 7 to PATH" Green
            }
            return $true
        }
    }
    return $false
}

# --- Main Execution ---

Write-ColorOutput "🔷 mylm Windows Installation Script" Magenta
Write-ColorOutput "================================" Magenta
Write-Host ""

# Detect if pwsh command is available
$pwshAvailable = Test-CommandExists "pwsh"

if (-not $pwshAvailable) {
    # Try to find and add PowerShell 7 to PATH
    Write-ColorOutput "🔍 Looking for PowerShell 7 installation..." Cyan
    $foundPS7 = Add-PowerShell7ToPath
    
    # Re-check if pwsh is now available
    $pwshAvailable = Test-CommandExists "pwsh"
    
    if (-not $pwshAvailable) {
        Write-ColorOutput "⚠️  PowerShell 7 not found or not in PATH." Yellow
        Write-Host ""
        Write-ColorOutput "If you installed PowerShell 7 via winget, run this in PowerShell 5.1 as Admin:" Cyan
        Write-Host "  [Environment]::SetEnvironmentVariable('PATH', \$env:PATH + ';C:\Program Files\PowerShell\7', 'User')" -ForegroundColor Gray
        Write-Host ""
        Write-Host "Then run this script with:" -ForegroundColor Yellow
        Write-Host "  pwsh -ExecutionPolicy Bypass -File install.ps1" -ForegroundColor Gray
        Write-Host ""
        Write-Host "OR run this script directly with PowerShell 5.1:" -ForegroundColor Yellow
        Write-Host "  powershell -ExecutionPolicy Bypass -File install.ps1" -ForegroundColor Gray
        Write-Host ""
        
        $usePS5 = Read-Host "Continue with PowerShell 5.1? [y/N]"
        if ($usePS5 -notmatch '^[Yy]$') {
            exit 0
        }
    }
}

# Check if running as administrator (not required but warn)
$isAdmin = $false
try {
    $isAdmin = (New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
} catch {
    # PS 5.1 might not support this fully
}

if ($isAdmin) {
    Write-ColorOutput "⚠️  WARNING: Running as Administrator. This is NOT required." Yellow
    Write-ColorOutput "   Running as admin may cause permission issues." Yellow
    $continue = Read-Host "Continue anyway? [y/N]"
    if ($continue -notmatch '^[Yy]$') {
        exit 0
    }
}

# Check PowerShell version
$psVersion = $PSVersionTable.PSVersion.Major
if ($psVersion -lt 7) {
    Write-ColorOutput "ℹ️  Running on PowerShell $psVersion (legacy version)" Cyan
    Write-ColorOutput "   PowerShell 7+ is recommended for best experience." Yellow
    Write-Host ""
}

# Main menu loop
while ($true) {
    Show-Menu
    $choice = Read-Host "Select an option [1-5]"
    
    switch ($choice) {
        "1" {
            Start-FreshInstallation
            Read-Host "Press Enter to return to menu..."
            Clear-Host
        }
        "2" {
            Start-UpdateExisting
            Read-Host "Press Enter to return to menu..."
            Clear-Host
        }
        "3" {
            Set-PowerShellAlias -Mandatory $false
            Read-Host "Press Enter to return to menu..."
            Clear-Host
        }
        "4" {
            if (Test-Path $BinaryDest) {
                Start-SetupWizard -Mandatory $false
            }
            else {
                Write-ColorOutput "❌ Error: Binary not found at $BinaryDest. Please install first." Red
            }
            Read-Host "Press Enter to return to menu..."
            Clear-Host
        }
        "5" {
            Write-ColorOutput "Goodbye!" Cyan
            exit 0
        }
        default {
            Write-ColorOutput "❌ Invalid option." Red
            Start-Sleep -Seconds 1
            Clear-Host
        }
    }
}
