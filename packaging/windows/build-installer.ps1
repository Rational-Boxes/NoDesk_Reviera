# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Reviera contributors
<#
.SYNOPSIS
    Builds NoDesk Reviera for Windows (x64): a portable ZIP and an Inno Setup installer.

.DESCRIPTION
    Run it on Windows 10/11 from the repository (any PowerShell; the Visual Studio build
    environment is loaded by the script):

        powershell -ExecutionPolicy Bypass -File packaging\windows\build-installer.ps1

    Steps: vcpkg dependencies -> IfcOpenShell 0.8.5 -> cargo build --release -> stage the exe,
    its DLLs, the samples and the notices -> dist\*-portable.zip -> dist\*-setup.exe.

    Prerequisites (install once):
      - Visual Studio 2022 (or Build Tools) with "Desktop development with C++"
      - Git, CMake 3.25+ and Ninja on PATH (both come with Visual Studio's C++ CMake tools)
      - rustup (https://rustup.rs); rust-toolchain.toml selects Rust 1.99, MSVC target
      - LLVM (winget install LLVM.LLVM): libclang for bindgen in wxdragon-sys
      - Strawberry Perl and NASM (winget install StrawberryPerl.StrawberryPerl NASM.NASM):
        the vendored OpenSSL that SQLCipher uses is built from source
      - vcpkg (git clone https://github.com/microsoft/vcpkg; .\bootstrap-vcpkg.bat), found
        through -VcpkgRoot or VCPKG_ROOT
      - Inno Setup 7 (or 6.3+) (winget install JRSoftware.InnoSetup) for the installer

.PARAMETER Version
    The release version, used in file names and shown by the installer.
.PARAMETER VcpkgRoot
    The vcpkg checkout (default: $env:VCPKG_ROOT, else C:\vcpkg).
.PARAMETER SkipDeps
    Do not run vcpkg install (the packages are already there).
.PARAMETER SkipIfcOpenShell
    Reuse third_party\install when it holds IfcOpenShell.
.PARAMETER NoInstaller
    Stop after the portable ZIP.
#>
[CmdletBinding()]
param(
    [string]$Version = '0.1.0-pre.1',
    [string]$VcpkgRoot = $(if ($env:VCPKG_ROOT) { $env:VCPKG_ROOT } else { 'C:\vcpkg' }),
    [switch]$SkipDeps,
    [switch]$SkipIfcOpenShell,
    [switch]$NoInstaller
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$Here = $PSScriptRoot
$Root = (Resolve-Path (Join-Path $Here '..\..')).Path
$Work = Join-Path $Here 'build'
$Stage = Join-Path $Work 'stage'
$Dist = Join-Path $Root 'dist'
$IfcosTag = 'ifcopenshell-python-0.8.5'
$IfcosSrc = Join-Path $Root 'third_party\IfcOpenShell'
$IfcosBuild = Join-Path $Root 'third_party\build\ifcopenshell-msvc'
$IfcosPrefix = Join-Path $Root 'third_party\install'

# OCCT, LuaJIT and Eigen as DLLs (x64-windows); Boost as static libraries with the DLL runtime
# (x64-windows-static-md), which is what IfcOpenShell's CMake asks for on Windows.
$DynTriplet = 'x64-windows'
$BoostTriplet = 'x64-windows-static-md'
$DynPorts = @('opencascade', 'luajit', 'eigen3', 'nlohmann-json')
$BoostLibs = @('date_time', 'iostreams', 'program_options', 'regex', 'system', 'thread')

function Step([string]$Message) { Write-Host "`n==> $Message" -ForegroundColor Cyan }

function Invoke-Native {
    param([string]$Exe, [string[]]$Arguments)
    & $Exe @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Exe failed with exit code $LASTEXITCODE" }
}

function Require([string]$Command, [string]$Hint) {
    if (-not (Get-Command $Command -ErrorAction SilentlyContinue)) {
        throw "$Command was not found on PATH. $Hint"
    }
}

# ------------------------------------------------------------------ Visual Studio environment
function Import-VsDevShell {
    if (Get-Command cl.exe -ErrorAction SilentlyContinue) { return }
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path $vswhere)) { throw 'Visual Studio 2022 (C++ workload) is required.' }
    $vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (-not $vs) { throw 'No Visual Studio installation with the C++ x64 tools was found.' }
    Import-Module (Join-Path $vs 'Common7\Tools\Microsoft.VisualStudio.DevShell.dll')
    Enter-VsDevShell -VsInstallPath $vs -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64' | Out-Null
}

Step 'Checking prerequisites'
Import-VsDevShell
Require git 'Install Git for Windows.'
Require cmake 'Install CMake (or the Visual Studio C++ CMake tools).'
Require ninja 'Install Ninja (or the Visual Studio C++ CMake tools).'
Require cargo 'Install rustup from https://rustup.rs.'
Require perl 'Install Strawberry Perl (OpenSSL for SQLCipher is built from source).'
Require nasm 'Install NASM (OpenSSL assembly).'
$llvmBin = if ($env:LIBCLANG_PATH) { $env:LIBCLANG_PATH } else { Join-Path $env:ProgramFiles 'LLVM\bin' }
if (-not (Test-Path (Join-Path $llvmBin 'libclang.dll'))) {
    throw "libclang.dll was not found in $llvmBin. Install LLVM or set LIBCLANG_PATH."
}
$vcpkg = Join-Path $VcpkgRoot 'vcpkg.exe'
if (-not (Test-Path $vcpkg)) { throw "vcpkg was not found at $VcpkgRoot (set -VcpkgRoot or VCPKG_ROOT)." }
$Dyn = Join-Path $VcpkgRoot "installed\$DynTriplet"
$BoostDir = Join-Path $VcpkgRoot "installed\$BoostTriplet"

# ------------------------------------------------------------------ 1. vcpkg dependencies
if (-not $SkipDeps) {
    Step "vcpkg: $($DynPorts -join ', ') ($DynTriplet); Boost ($BoostTriplet)"
    Invoke-Native $vcpkg (@('install', '--recurse') + ($DynPorts | ForEach-Object { "${_}:$DynTriplet" }))
    $boostPorts = $BoostLibs | ForEach-Object { 'boost-' + ($_ -replace '_', '-') + ":$BoostTriplet" }
    Invoke-Native $vcpkg (@('install', '--recurse') + $boostPorts)
}
$OccInclude = Join-Path $Dyn 'include\opencascade'
$OccLib = Join-Path $Dyn 'lib'
if (-not (Test-Path (Join-Path $OccInclude 'Standard_Version.hxx'))) { throw "OCCT headers are missing under $OccInclude." }

# ------------------------------------------------------------------ 2. IfcOpenShell 0.8.5
# The options of tools/build-ifcopenshell.sh (keep the two in step), with the vcpkg toolchain.
if (-not ($SkipIfcOpenShell -and (Test-Path (Join-Path $IfcosPrefix 'include\ifcparse\IfcFile.h')))) {
    Step "IfcOpenShell ($IfcosTag)"
    if (-not (Test-Path (Join-Path $IfcosSrc '.git'))) {
        Invoke-Native git @('clone', '--depth', '1', '--branch', $IfcosTag, 'https://github.com/IfcOpenShell/IfcOpenShell.git', $IfcosSrc)
    }
    Invoke-Native cmake @(
        '-S', (Join-Path $IfcosSrc 'cmake'), '-B', $IfcosBuild, '-G', 'Ninja',
        '-DCMAKE_BUILD_TYPE=Release',
        "-DCMAKE_INSTALL_PREFIX=$IfcosPrefix",
        '-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreadedDLL',
        '-DCMAKE_POSITION_INDEPENDENT_CODE=ON',
        '-DBUILD_SHARED_LIBS=OFF',
        '-DSCHEMA_VERSIONS=2x3;4;4x3_add2',
        '-DBUILD_IFCPYTHON=OFF',
        '-DBUILD_CONVERT=ON',
        '-DBUILD_GEOMSERVER=OFF',
        '-DWITH_OPENCASCADE=ON',
        '-DWITH_CGAL=OFF',
        '-DCOLLADA_SUPPORT=OFF',
        '-DGLTF_SUPPORT=OFF',
        '-DHDF5_SUPPORT=OFF',
        '-DIFCXML_SUPPORT=OFF',
        '-DUSE_CCACHE=OFF',
        "-DOCC_INCLUDE_DIR=$OccInclude",
        "-DOCC_LIBRARY_DIR=$OccLib",
        "-DBOOST_ROOT=$BoostDir",
        "-DCMAKE_PREFIX_PATH=$BoostDir;$Dyn",
        "-DEIGEN_DIR=$(Join-Path $Dyn 'include\eigen3')"
    )
    Invoke-Native cmake @('--build', $IfcosBuild, '--parallel', "$env:NUMBER_OF_PROCESSORS")
    Invoke-Native cmake @('--install', $IfcosBuild)
}
$IfcosLib = Join-Path $IfcosPrefix 'lib'

# reviera-ifc's build script was written for GCC/Clang. Until it learns MSVC:
# - it picks the library folder holding libIfcParse.a: a marker file stands in for it (rustc then
#   links IfcParse.lib from the same folder);
# - it links boost_<name> by plain name: copies of vcpkg's versioned Boost libraries are put
#   there under those names (an empty boost_system.lib when Boost no longer builds one);
# - it passes `-isystem <dir>`, which cl.exe ignores: the same folders go on INCLUDE instead.
Step 'Preparing the link folder for reviera-ifc'
New-Item -ItemType File -Force (Join-Path $IfcosLib 'libIfcParse.a') | Out-Null
foreach ($name in $BoostLibs) {
    $target = Join-Path $IfcosLib "boost_$name.lib"
    $found = Get-ChildItem (Join-Path $BoostDir 'lib') -Filter "boost_$name*.lib" -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($found) {
        Copy-Item $found.FullName $target -Force
    } elseif ($name -eq 'system') {
        $empty = Join-Path $Work 'empty.c'
        New-Item -ItemType Directory -Force $Work | Out-Null
        Set-Content -Path $empty -Value 'static int reviera_boost_system_stub;'
        Push-Location $Work
        try {
            Invoke-Native cl.exe @('/nologo', '/c', $empty, '/Foempty.obj')
            Invoke-Native lib.exe @('/nologo', "/OUT:$target", 'empty.obj')
        } finally { Pop-Location }
    } else {
        throw "boost_$name was not found under $BoostDir\lib."
    }
}

# ------------------------------------------------------------------ 3. cargo build --release
Step 'cargo build --release'
$env:OCC_INCLUDE_DIR = $OccInclude
$env:OCC_LIBRARY_DIR = $OccLib
$env:REVIERA_IFCOS_PREFIX = $IfcosPrefix
$env:LIBCLANG_PATH = $llvmBin
$env:LUA_LIB = Join-Path $Dyn 'lib'
$env:LUA_LIB_NAME = 'lua51'
$env:LUA_LINK = 'dylib'
$env:INCLUDE = @(
    (Join-Path $IfcosPrefix 'include'),
    $OccInclude,
    (Join-Path $Dyn 'include\eigen3'),
    (Join-Path $BoostDir 'include'),
    (Join-Path $Dyn 'include'),
    $env:INCLUDE
) -join ';'
Push-Location $Root
try {
    Invoke-Native cargo @('build', '--release', '--locked', '-p', 'reviera')
} finally { Pop-Location }
$Exe = Join-Path $Root 'target\release\reviera.exe'

# ------------------------------------------------------------------ 4. Stage
Step "Staging $Stage"
if (Test-Path $Stage) { Remove-Item -Recurse -Force $Stage }
New-Item -ItemType Directory -Force $Stage | Out-Null
Copy-Item $Exe $Stage

# The DLLs reviera.exe needs from vcpkg, followed recursively (dumpbin /dependents).
$searchDirs = @((Join-Path $Dyn 'bin'), (Join-Path $BoostDir 'bin'))
$seen = @{}
$queue = New-Object System.Collections.Generic.Queue[string]
$queue.Enqueue($Exe)
while ($queue.Count -gt 0) {
    $file = $queue.Dequeue()
    $deps = & dumpbin.exe /nologo /dependents $file |
        ForEach-Object { $_.Trim() } | Where-Object { $_ -match '^[^\\/:]+\.dll$' }
    foreach ($dll in $deps) {
        $key = $dll.ToLowerInvariant()
        if ($seen.ContainsKey($key)) { continue }
        $seen[$key] = $true
        foreach ($dir in $searchDirs) {
            $candidate = Join-Path $dir $dll
            if (Test-Path $candidate) {
                Copy-Item $candidate $Stage
                $queue.Enqueue($candidate)
                break
            }
        }
        # Anything not found in vcpkg is a Windows system DLL (or the MSVC runtime, below).
    }
}
Write-Host ("Bundled DLLs: " + ((Get-ChildItem $Stage -Filter *.dll | ForEach-Object Name) -join ', '))

# The MSVC runtime, app-local, so the ZIP also runs where the VC++ redistributable is missing.
$crt = Get-ChildItem (Join-Path $env:VCToolsRedistDir 'x64') -Directory -Filter 'Microsoft.VC*.CRT' -ErrorAction SilentlyContinue |
    Select-Object -First 1
if ($crt) { Copy-Item (Join-Path $crt.FullName '*.dll') $Stage }

# Samples beside the exe (crates/reviera-ui/src/help.rs: <exe dir>\samples).
$samples = Join-Path $Stage 'samples'
New-Item -ItemType Directory -Force $samples | Out-Null
$sampleSrc = Join-Path $Root 'test-data\samples'
Copy-Item (Join-Path $sampleSrc 'samples.json') $samples
foreach ($pattern in @('*.ifc', '*.lua', '*.md')) { Copy-Item (Join-Path $sampleSrc $pattern) $samples }
# The sample Component Library beside the samples (crates/reviera-ui/src/components.rs).
Copy-Item -Recurse -Force (Join-Path $Root 'test-data\library') (Join-Path $Stage 'library')
Copy-Item (Join-Path $Root 'LICENSE') (Join-Path $Stage 'COPYING.txt')
Copy-Item (Join-Path $Root 'packaging\THIRD-PARTY-NOTICES.txt') $Stage
Copy-Item (Join-Path $Root 'packaging\icons\reviera.ico') $Stage
Set-Content -Path (Join-Path $Stage 'NoDesk Reviera.cmd') -Encoding ASCII -Value @(
    '@echo off',
    'rem Opens the Reviera window (reviera.exe without "gui" is the command-line tool).',
    'start "" "%~dp0reviera.exe" gui %*'
)

# ------------------------------------------------------------------ 5. Portable ZIP
New-Item -ItemType Directory -Force $Dist | Out-Null
$zip = Join-Path $Dist "NoDesk-Reviera-$Version-windows-x64-portable.zip"
Step "Portable ZIP $zip"
if (Test-Path $zip) { Remove-Item $zip }
$zipRoot = Join-Path $Work "NoDesk-Reviera-$Version"
if (Test-Path $zipRoot) { Remove-Item -Recurse -Force $zipRoot }
Copy-Item $Stage $zipRoot -Recurse
Compress-Archive -Path $zipRoot -DestinationPath $zip
Remove-Item -Recurse -Force $zipRoot

# ------------------------------------------------------------------ 6. Installer
if (-not $NoInstaller) {
    $iscc = (Get-Command iscc.exe -ErrorAction SilentlyContinue).Source
    if (-not $iscc) {
        $iscc = foreach ($v in @('7', '6')) {
            (Join-Path $env:ProgramFiles "Inno Setup $v\ISCC.exe")
            (Join-Path ${env:ProgramFiles(x86)} "Inno Setup $v\ISCC.exe")
            (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup $v\ISCC.exe")
        }
        $iscc = $iscc | Where-Object { Test-Path $_ } | Select-Object -First 1
    }
    if (-not $iscc) { throw 'Inno Setup 7 or 6 (ISCC.exe) was not found; install it or pass -NoInstaller.' }
    # Inno Setup's version fields take numbers only.
    $numeric = ($Version -split '-')[0]
    Step "Installer (Inno Setup)"
    Invoke-Native $iscc @(
        "/DAppVersion=$Version",
        "/DAppVersionNumeric=$numeric",
        "/DStageDir=$Stage",
        "/DOutputDir=$Dist",
        (Join-Path $Here 'reviera.iss')
    )
}

Step 'Done'
Get-ChildItem $Dist -Filter "NoDesk-Reviera-$Version-windows-*" | Format-Table Name, Length
