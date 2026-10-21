# Measures this PC and the app itself, and writes docs/data/benchmarks.json
# for the website. Safe to run: scans are read-only, the window and tray copies
# it starts use a throwaway data folder (OCO_DATA_DIR) and change nothing
# (OCO_DRY_RUN), and the Focus test only slows processes it starts itself.
#
#   cargo build --release
#   powershell -ExecutionPolicy Bypass -File tools\bench.ps1
param([string]$Out = (Join-Path $PSScriptRoot '..\docs\data\benchmarks.json'))

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
$bin = Join-Path $root 'target\release'
$app = Join-Path $bin 'OneClickOptimizer.exe'
$cli = Join-Path $bin 'oneclick-cli.exe'
Add-Type @'
using System; using System.Runtime.InteropServices; using System.Text;
public static class B {
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  public static IntPtr Find(string cls, uint pid) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); var sb = new StringBuilder(128); GetClassName(h, sb, 128);
      if (p == pid && sb.ToString() == cls) { found = h; return false; } return true; }, IntPtr.Zero);
    return found;
  }
}
'@

function Sandboxed([string[]]$argv) {
  $data = Join-Path ([IO.Path]::GetTempPath()) ("oco-bench-" + [guid]::NewGuid())
  New-Item -ItemType Directory $data | Out-Null
  $env:OCO_DATA_DIR = $data; $env:OCO_OFFSCREEN = '1'; $env:OCO_DRY_RUN = '1'; $env:OCO_NO_UPDATE_CHECK = '1'
  try { if ($argv) { Start-Process $app -ArgumentList $argv -PassThru } else { Start-Process $app -PassThru } }
  finally { foreach ($k in 'OCO_DATA_DIR', 'OCO_OFFSCREEN', 'OCO_DRY_RUN', 'OCO_NO_UPDATE_CHECK') { Remove-Item "env:$k" } }
}
function PrivateMB($p) { $p.Refresh(); [math]::Round($p.PrivateMemorySize64 / 1MB, 1) }

Write-Host 'Scanning (read-only) and timing the Focus test...'
$bench = & $cli bench --json | Out-String | ConvertFrom-Json

Write-Host 'Measuring the window...'
$w = Sandboxed @()
$h = [IntPtr]::Zero
for ($i = 0; $i -lt 50 -and $h -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 200; $h = [B]::Find('OneClickOptimizerWindow', [uint32]$w.Id) }
Start-Sleep -Seconds 3
$windowIdle = PrivateMB $w
[B]::PostMessage($h, 0x111, [IntPtr]210, [IntPtr]::Zero) | Out-Null   # Scan now
Start-Sleep -Seconds 8
$windowScanned = PrivateMB $w
Stop-Process -Id $w.Id -Force

Write-Host 'Measuring the tray watch...'
$a = Sandboxed @('agent')
Start-Sleep -Seconds 10
$trayIdle = PrivateMB $a
Stop-Process -Id $a.Id -Force

$os = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
$result = [ordered]@{
  measured = (Get-Date).ToString('yyyy-MM-dd')
  version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
  windows = "Windows 11 build $($os.CurrentBuild)"
  machine = $bench.machine
  memory = $bench.memory
  footprint = [ordered]@{
    exe_bytes = (Get-Item $app).Length
    window_idle_mb = $windowIdle
    window_after_scan_mb = $windowScanned
    tray_mb = $trayIdle
    scan_ms = $bench.scan.ms
  }
  scan = $bench.scan
  dev = $bench.dev
  focus = $bench.focus
}
New-Item -ItemType Directory -Force (Split-Path $Out) | Out-Null
$json = $result | ConvertTo-Json -Depth 8
[IO.File]::WriteAllText([IO.Path]::GetFullPath($Out), $json, (New-Object Text.UTF8Encoding($false)))
# The page loads the same data as a script, so it also works opened from disk.
[IO.File]::WriteAllText([IO.Path]::GetFullPath([IO.Path]::ChangeExtension($Out, '.js')), "window.BENCH = $json;`n", (New-Object Text.UTF8Encoding($false)))
Write-Host "Wrote $Out"
$result.footprint | Format-List
