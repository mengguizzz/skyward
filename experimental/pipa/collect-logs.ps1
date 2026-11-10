param(
    [string]$Serial = ""
)

$ErrorActionPreference = "Stop"
$adbCommand = (Get-Command adb -ErrorAction Stop).Source
$deviceArgs = @()
if ($Serial) {
    $deviceArgs = @("-s", $Serial)
}
$logDirectory = Join-Path $PSScriptRoot ("logs\" + (Get-Date -Format "yyyyMMdd-HHmmss"))
New-Item -ItemType Directory -Path $logDirectory -Force | Out-Null

& $adbCommand @deviceArgs get-state
if ($LASTEXITCODE -ne 0) {
    throw "ADB device is unavailable. Check the USB connection and recovery boot."
}
& $adbCommand @deviceArgs shell test -f /skyward/device.toml
if ($LASTEXITCODE -ne 0) {
    throw "Skyward recovery is not running. Temporarily boot the test image before collecting these logs."
}
& $adbCommand @deviceArgs pull /skyward/run/recovery.log (Join-Path $logDirectory "recovery.log")
& $adbCommand @deviceArgs shell test -f /skyward/run/supplicant/supplicant.log
if ($LASTEXITCODE -eq 0) {
    & $adbCommand @deviceArgs pull /skyward/run/supplicant/supplicant.log (Join-Path $logDirectory "supplicant.log")
}
& $adbCommand @deviceArgs shell logcat -d -t 2000 2>&1 | Out-File -LiteralPath (Join-Path $logDirectory "logcat.txt") -Encoding utf8
& $adbCommand @deviceArgs shell dmesg 2>&1 | Out-File -LiteralPath (Join-Path $logDirectory "dmesg.txt") -Encoding utf8
& $adbCommand @deviceArgs shell getprop 2>&1 | Out-File -LiteralPath (Join-Path $logDirectory "getprop.txt") -Encoding utf8
& $adbCommand @deviceArgs shell cat /proc/cmdline 2>&1 | Out-File -LiteralPath (Join-Path $logDirectory "cmdline.txt") -Encoding utf8
& $adbCommand @deviceArgs shell ls -l /dev/dri /sys/class/backlight 2>&1 | Out-File -LiteralPath (Join-Path $logDirectory "display-nodes.txt") -Encoding utf8
& $adbCommand @deviceArgs shell 'ls /sys/class/net; cat /proc/modules; ps -A; cat /proc/mounts' 2>&1 | Out-File -LiteralPath (Join-Path $logDirectory "services-wlan-mounts.txt") -Encoding utf8
& $adbCommand @deviceArgs shell 'cat /sys/fs/pstore/console-ramoops* 2>/dev/null' 2>&1 | Out-File -LiteralPath (Join-Path $logDirectory "previous-kernel.txt") -Encoding utf8
Write-Output "Logs saved to $logDirectory"
