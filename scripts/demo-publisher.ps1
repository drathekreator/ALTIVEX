# =====================================================================
# ALTIVEX Demo Publisher (mosquitto_pub simulator)
# ---------------------------------------------------------------------
# Simulasi 1 ESP32 demo di terminal Windows pakai mosquitto_pub.
# Generate posisi yang ngikutin loop CIFOR-Situgede (waypoint sama
# persis dengan altivex_demo_situgede.ino), publish ke broker demo.
#
# Cara pakai:
#   .\scripts\demo-publisher.ps1                          # default DEMO-CIFOR-01
#   .\scripts\demo-publisher.ps1 -DeviceId DEMO-CIFOR-02  # device lain
#   .\scripts\demo-publisher.ps1 -DeviceId TEST -LoopMin 2  # loop 2 menit
#
# Prerequisite: mosquitto_pub.exe ter-install di
#   "C:\Program Files\mosquitto\mosquitto_pub.exe"
# =====================================================================

[CmdletBinding()]
param(
    [string]$DeviceId       = 'DEMO-CIFOR-01',
    [string]$Broker         = 'altivex-demo.duckdns.org',
    [int]   $Port           = 1885,
    [string]$Username       = 'altivex_demo',
    [string]$Password       = $env:ALTIVEX_DEMO_MQTT_PASSWORD,
    [string]$Topic          = 'altivex/sensor/data',
    [int]   $IntervalSec    = 3,        # frekuensi publish
    [int]   $LoopMin        = 10,       # 1 loop selesai berapa menit
    [string]$MosquittoPub   = 'C:\Program Files\mosquitto\mosquitto_pub.exe',
    [switch]$DryRun                     # print payload tanpa publish
)

$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------------
# Validasi
# ---------------------------------------------------------------------
if (-not $DryRun) {
    if (-not (Test-Path $MosquittoPub)) {
        Write-Error "mosquitto_pub.exe tidak ditemukan di '$MosquittoPub'. Install dari https://mosquitto.org/download/ atau set -MosquittoPub <path>."
    }
    if (-not $Password) {
        Write-Error @"
MQTT password belum di-set.

Pilihan:
  1. Set env variable sekali per session:
       `$env:ALTIVEX_DEMO_MQTT_PASSWORD = '<paste dari .env.demo>'

  2. Atau pass langsung:
       .\scripts\demo-publisher.ps1 -Password '<paste>'

Cara ambil password dari VM:
  ssh user@vm "grep MQTT_PASSWORD ~/ALTIVEX/deployment/demo-branch/.env.demo"
"@
    }
}

# ---------------------------------------------------------------------
# Waypoints loop CIFOR-Situgede (match altivex_demo_situgede.ino)
# ---------------------------------------------------------------------
$waypoints = @(
    @{ lng = 106.7518232; lat = -6.5546282 }   # Jl. CIFOR start
    @{ lng = 106.7510000; lat = -6.5540000 }
    @{ lng = 106.7498000; lat = -6.5532000 }
    @{ lng = 106.7482000; lat = -6.5524000 }
    @{ lng = 106.7469000; lat = -6.5519000 }
    @{ lng = 106.7457227; lat = -6.5517073 }   # Jl. Cilubang Malang
    @{ lng = 106.7462000; lat = -6.5524000 }
    @{ lng = 106.7470000; lat = -6.5532000 }
    @{ lng = 106.7480000; lat = -6.5540000 }
    @{ lng = 106.7490000; lat = -6.5547000 }
    @{ lng = 106.7500000; lat = -6.5550000 }
    @{ lng = 106.7507053; lat = -6.5551558 }   # Warung Tepi Hutan
    @{ lng = 106.7510000; lat = -6.5549000 }
    @{ lng = 106.7515000; lat = -6.5547000 }
    @{ lng = 106.7518232; lat = -6.5546282 }   # closed loop
)
$segCount = $waypoints.Count - 1

# ---------------------------------------------------------------------
# Helper: posisi pada progress (0.0 -> 1.0) di loop, linear interp
# antara dua waypoint terdekat.
# ---------------------------------------------------------------------
function Get-LoopPosition([double]$progress) {
    if ($progress -lt 0) { $progress = 0 }
    if ($progress -ge 1) { $progress = $progress - [math]::Floor($progress) }
    $segFrac = $progress * $segCount
    $segIdx  = [int][math]::Floor($segFrac)
    if ($segIdx -ge $segCount) { $segIdx = $segCount - 1 }
    $t = $segFrac - $segIdx
    $a = $waypoints[$segIdx]
    $b = $waypoints[$segIdx + 1]
    return @{
        lng = $a.lng + ($b.lng - $a.lng) * $t
        lat = $a.lat + ($b.lat - $a.lat) * $t
    }
}

# ---------------------------------------------------------------------
# Banner
# ---------------------------------------------------------------------
Write-Host ""
Write-Host "============================================================"
Write-Host "ALTIVEX Demo Publisher" -ForegroundColor Cyan
Write-Host "============================================================"
Write-Host "Device:    $DeviceId"
Write-Host "Broker:    $Broker`:$Port"
Write-Host "Topic:     $Topic"
Write-Host "Interval:  $IntervalSec sec"
Write-Host "Loop:      $LoopMin minutes per round"
Write-Host "Mode:      $(if ($DryRun) {'DRY RUN (no publish)'} else {'LIVE PUBLISH'})"
Write-Host "============================================================"
Write-Host "Tekan Ctrl+C untuk stop." -ForegroundColor Yellow
Write-Host ""

$loopMs   = $LoopMin * 60 * 1000
$startMs  = [int64]([System.Diagnostics.Stopwatch]::GetTimestamp() / [System.Diagnostics.Stopwatch]::Frequency * 1000)
$count    = 0

while ($true) {
    $nowMs   = [int64]([System.Diagnostics.Stopwatch]::GetTimestamp() / [System.Diagnostics.Stopwatch]::Frequency * 1000)
    $elapsed = $nowMs - $startMs
    $progress = ($elapsed % $loopMs) / [double]$loopMs

    $pos = Get-LoopPosition $progress

    # Battery decay: 100 -> 20, drop 1% per 30 sec, floor 20
    $batteryDrop = [int]([math]::Floor($elapsed / 30000))
    $battery = 100 - $batteryDrop
    if ($battery -lt 20) { $battery = 20 }

    # JSON payload — match struct IncomingData di backend Rust
    $payload = '{{"id_perangkat":"{0}","latitude":{1:F6},"longitude":{2:F6},"battery":{3}}}' -f `
        $DeviceId, $pos.lat, $pos.lng, $battery

    $count++
    $stamp = (Get-Date).ToString('HH:mm:ss')
    $progressPct = [math]::Round($progress * 100, 1)
    Write-Host ("[{0}] #{1,-4} loop={2,5}% bat={3,3}% -> {4}" -f `
        $stamp, $count, $progressPct, $battery, $payload)

    if (-not $DryRun) {
        & $MosquittoPub `
            -h $Broker -p $Port `
            -u $Username -P $Password `
            -t $Topic `
            -q 1 `
            -m $payload 2>$null

        if ($LASTEXITCODE -ne 0) {
            Write-Host "  ⚠️  mosquitto_pub exit code=$LASTEXITCODE" -ForegroundColor Red
        }
    }

    Start-Sleep -Seconds $IntervalSec
}
