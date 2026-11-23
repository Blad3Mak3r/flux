# Downloads the FFmpeg build that FLUX links and ships on Windows into src-tauri/ffmpeg: a shared
# LGPL build with headers, import libraries and DLLs. Building needs FFMPEG_DIR pointing there; the
# installer takes the DLLs and the licence from it (src-tauri/tauri.windows.conf.json).
#
#   pwsh scripts/fetch-ffmpeg.ps1
#   $env:FFMPEG_DIR = "$PWD\src-tauri\ffmpeg"
$ErrorActionPreference = 'Stop'

$name = 'ffmpeg-n8.1-latest-win64-lgpl-shared-8.1'
$target = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\src-tauri\ffmpeg'))

if (-not (Test-Path (Join-Path $target 'bin\avcodec-*.dll'))) {
  $temp = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [System.IO.Path]::GetTempPath() }
  $zip = Join-Path $temp "$name.zip"
  Invoke-WebRequest "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/$name.zip" -OutFile $zip
  Expand-Archive $zip -DestinationPath $temp -Force
  if (Test-Path $target) { Remove-Item $target -Recurse -Force }
  Move-Item (Join-Path $temp $name) $target
  Remove-Item $zip
}

if ($env:GITHUB_ENV) {
  "FFMPEG_DIR=$target" | Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8
  Join-Path $target 'bin' | Out-File -FilePath $env:GITHUB_PATH -Append -Encoding utf8
} else {
  Write-Host "FFmpeg is in $target. Before building, set:"
  Write-Host "  `$env:FFMPEG_DIR = '$target'"
}
