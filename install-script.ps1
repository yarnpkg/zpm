#!/usr/bin/env pwsh
# Windows counterpart of install-script.sh. Usage:
#
#   powershell -c "irm https://repo.yarnpkg.com/install.ps1 | iex"
#
# Or, to pass options:
#
#   & ([scriptblock]::Create((irm https://repo.yarnpkg.com/install.ps1))) -Canary

param(
  # Install the latest canary release rather than the latest stable one
  [switch]$Canary,

  # Copy the binary into this folder instead of the default location, and
  # don't modify the PATH
  [string]$BinDir,

  # Specific version to install (e.g. "6.0.0")
  [Parameter(Position = 0)]
  [string]$Version
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

function Write-Yarn([string]$Text) {
  Write-Host $Text -ForegroundColor Cyan -NoNewline
}

# This script is usually piped into `iex`, so we must never call `exit`, as it
# would close the user's terminal; throwing stops the script instead.
function Fail([string]$Message) {
  throw "Error: $Message"
}

# Windows on ARM runs x64 binaries through emulation
$target = 'x86_64-pc-windows-msvc'

$installChannel = if ($Canary) { 'canary' } else { 'stable' }

$installDir = Join-Path $HOME '.yarn\switch\bin'
$tmpDir = "$installDir.tmp"
$archive = Join-Path $tmpDir 'yarn.zip'

if (Test-Path $tmpDir) {
  Remove-Item -Recurse -Force $tmpDir
}

New-Item -ItemType Directory -Force -Path $tmpDir | Out-Null

if (-not $Version) {
  $Version = (Invoke-RestMethod -Uri "https://repo.yarnpkg.com/channels/default/$installChannel").Trim()
}

$yarnUri = "https://repo.yarnpkg.com/releases/$Version/$target"

Write-Host 'This script will install or update ' -NoNewline
Write-Yarn 'Yarn Switch'
Write-Host ', a utility that lets you lock Yarn versions in your projects.'
Write-Host 'For more information, please take a look at our documentation at https://yarnpkg.com'
Write-Host

try {
  Invoke-WebRequest -Uri $yarnUri -OutFile $archive -UseBasicParsing
} catch {
  Fail "Failed to download Yarn from $yarnUri"
}

Expand-Archive -Path $archive -DestinationPath $tmpDir -Force
Remove-Item -Force (Join-Path $tmpDir 'yarn-bin.exe')
Remove-Item -Force $archive

if ($BinDir) {
  Move-Item -Force (Join-Path $tmpDir 'yarn.exe') $BinDir
  Remove-Item -Recurse -Force $tmpDir
  return
}

if (Test-Path $installDir) {
  Remove-Item -Recurse -Force $installDir
}

Move-Item $tmpDir $installDir

# Add the install folder to the user PATH. We go through the registry rather
# than [Environment]::SetEnvironmentVariable so that entries referencing other
# variables (stored as REG_EXPAND_SZ) don't get expanded in the process.
$environmentKey = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
$userPath = $environmentKey.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)

$pathEntries = $userPath -split ';' | Where-Object { $_ -ne '' }
if ($pathEntries -notcontains $installDir) {
  $environmentKey.SetValue('Path', (@($installDir) + $pathEntries) -join ';', [Microsoft.Win32.RegistryValueKind]::ExpandString)

  # Setting a variable through .NET broadcasts WM_SETTINGCHANGE, which lets
  # Explorer (and the terminals it spawns) pick up the new PATH
  [Environment]::SetEnvironmentVariable('YARN_SWITCH_INSTALL', '1', 'User')
  [Environment]::SetEnvironmentVariable('YARN_SWITCH_INSTALL', $null, 'User')
}

$environmentKey.Close()

$env:Path = "$installDir;$env:Path"

Write-Host

& (Join-Path $installDir 'yarn.exe') switch postinstall -H $HOME
