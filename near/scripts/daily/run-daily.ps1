# Scheduled task "Warband daily raid" (current user, 12:00 and 23:00 and at logon, until 2026-11-26): the gas-key
# keeper's upkeep, then the camp joins today's raid. Everything is appended to near/scripts/daily/daily.log.
# Keys stay in the WSL legacy keychain (~/.near-credentials/testnet); nothing secret is written to the log.
$ErrorActionPreference = "Continue"
# WSL writes UTF-8: read it as UTF-8, or box characters from near-cli turn into mojibake in the log
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$log = Join-Path $here "daily.log"
$keychain = "\\wsl.localhost\Ubuntu-26.04\home\byezz\.near-credentials\testnet"
$mutex = New-Object System.Threading.Mutex($false, "WarbandDailyRaid")
if (-not $mutex.WaitOne(0)) { exit 0 }   # a run is already going
try {
  Push-Location (Join-Path $here "..\..\agent")
  $env:GAS_KEEPER = "gk-daily.warband-hq.testnet"; $env:WARBAND = "warband-daily.testnet"; $env:RPC = "https://rpc.testnet.fastnear.com"
  "" | Out-File -Append -Encoding utf8 $log
  "== $((Get-Date).ToUniversalTime().ToString('yyyy-MM-dd HH:mm:ss')) UTC, run-daily.ps1" | Out-File -Append -Encoding utf8 $log
  & npx.cmd tsx src/daily-keeper.ts $keychain 2>&1 | Out-File -Append -Encoding utf8 $log
  Pop-Location
  $sh = "/mnt/c/Users/byezz/Desktop/hackathon/near/warband-near/near/scripts/daily/play-today.sh"
  & wsl.exe -d Ubuntu-26.04 -e bash -c "export PATH=`$HOME/.cargo/bin:`$PATH; bash $sh" 2>&1 |
    Where-Object { $_ -notmatch "^wsl:" } | Out-File -Append -Encoding utf8 $log
} finally { $mutex.ReleaseMutex() }
