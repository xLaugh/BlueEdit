# Builds Blue Edit for Windows into dist\windows:
#   BlueEdit-<version>-windows-portable.exe   the program alone (ffmpeg taken from the PATH)
#   BlueEdit-<version>-Setup.exe              installer (Inno Setup 6), ffmpeg included with -WithFfmpeg
# Needs Rust (MSVC toolchain); Inno Setup 6 for the installer.
param(
    [switch]$WithFfmpeg,
    [string]$FfmpegVersion = "8.1"
)
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

cargo build --release --locked
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
$version = (Select-String -Path Cargo.toml -Pattern '^version' | Select-Object -First 1).Line.Split('"')[1]
$dist = "dist\windows"
New-Item -ItemType Directory -Force $dist | Out-Null

Copy-Item target\release\blue.exe "$dist\blue.exe" -Force
Copy-Item target\release\blue.exe "$dist\BlueEdit-$version-windows-portable.exe" -Force
Write-Host "==> $dist\BlueEdit-$version-windows-portable.exe"

$isccArgs = @("/Q", "/DAppVersion=$version", "/DExe=..\$dist\blue.exe")
if ($WithFfmpeg) {
    # Windows builds of ffmpeg by Gyan Doshi (GPL), fixed version.
    $name = "ffmpeg-$FfmpegVersion-essentials_build"
    $zip = "$dist\$name.zip"
    if (-not (Test-Path $zip)) {
        Invoke-WebRequest "https://github.com/GyanD/codexffmpeg/releases/download/$FfmpegVersion/$name.zip" -OutFile $zip
    }
    $tmp = "$dist\ffmpeg-extract"
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    Expand-Archive $zip -DestinationPath $tmp
    $ff = "$dist\ffmpeg"
    New-Item -ItemType Directory -Force $ff | Out-Null
    Copy-Item "$tmp\$name\bin\ffmpeg.exe", "$tmp\$name\bin\ffprobe.exe" $ff -Force
    Copy-Item "$tmp\$name\LICENSE" "$ff\LICENSE-ffmpeg.txt" -Force
    Remove-Item -Recurse -Force $tmp
    $isccArgs += "/DFfmpegDir=..\$ff"
}

$iscc = @(
    (Get-Command iscc -ErrorAction SilentlyContinue).Source,
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if ($iscc) {
    & $iscc @isccArgs installer\blueedit.iss
    if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed" }
    Write-Host "==> $dist\BlueEdit-$version-Setup.exe"
} else {
    Write-Host "!! Inno Setup 6 not found: installer not built"
}
