# Rebuild GEO.json untuk demo altivex-demo.duckdns.org dengan rute jalan
# lurus 32.6m di lingkungan kampus, buffer ±20m untuk toleransi error GPS.
#
# Sumber koordinat: deployment/demo-branch/frontend-override/
# straight_road_geofence.json (referensi user).
#
# Buffer ditingkatkan dari ±10m (referensi) ke ±20m untuk:
#   - GPS NEO-6M tipikal 2-5m error outdoor
#   - Drift di bawah kanopi pohon kampus bisa 5-10m
#   - Margin tambahan supaya saat presentasi marker tidak flicker
#     in/out polygon karena mikro-jitter GPS
#
# Output: deployment/demo-branch/frontend-override/GEO.json

$ErrorActionPreference = 'Stop'

# Origin & destination dari referensi user
$origin = @{ lat = -6.589791; lng = 106.806552 }
$dest   = @{ lat = -6.590063; lng = 106.806443 }

# Konversi 1 deg ke meter (di lat ≈ -6.59)
$latRef    = $origin.lat
$mPerDegLat = 111000.0
$mPerDegLng = 111000.0 * [Math]::Cos($latRef * [Math]::PI / 180.0)

# Vector route dalam meter (lokal flat-earth)
$dx = ($dest.lng - $origin.lng) * $mPerDegLng
$dy = ($dest.lat - $origin.lat) * $mPerDegLat
$lenM = [Math]::Sqrt($dx * $dx + $dy * $dy)
$lenRound = [Math]::Round($lenM, 2)

Write-Host "Route length: $lenRound m"

# Bearing (untuk metadata)
$bearingRad = [Math]::Atan2($dx, $dy)
$bearing = ($bearingRad * 180.0 / [Math]::PI + 360) % 360
$bearingRound = [Math]::Round($bearing, 1)

# Unit vector + perpendicular (rotasi 90° CCW)
$ux = $dx / $lenM
$uy = $dy / $lenM
$perpX = -$uy   # rotasi 90° CCW: (x,y) -> (-y, x)
$perpY = $ux

# Buffer 20m
$bufferM = 20.0
$offX = $perpX * $bufferM
$offY = $perpY * $bufferM

# Konversi offset balik ke degree
$offDegLng = $offX / $mPerDegLng
$offDegLat = $offY / $mPerDegLat

# 4 sudut polygon (counterclockwise saat dilihat dari atas):
#   P1: origin + side A
#   P2: dest   + side A
#   P3: dest   + side B (= -A)
#   P4: origin + side B
$p1 = @{ lng = $origin.lng + $offDegLng; lat = $origin.lat + $offDegLat }
$p2 = @{ lng = $dest.lng   + $offDegLng; lat = $dest.lat   + $offDegLat }
$p3 = @{ lng = $dest.lng   - $offDegLng; lat = $dest.lat   - $offDegLat }
$p4 = @{ lng = $origin.lng - $offDegLng; lat = $origin.lat - $offDegLat }

# Format helper
function Fmt($v) { return ('{0:0.0000000}' -f $v) }

# Build JSON manual (PowerShell ConvertTo-Json suka flatten array)
$json = @"
{
  "type": "FeatureCollection",
  "name": "Geofence - Jalan Lurus Kampus",
  "metadata": {
    "description": "Rute lurus pengujian di lingkungan kampus (Bogor). Buffer geofence ±${bufferM}m untuk mengakomodasi error GPS NEO-6M.",
    "source": "Manual koordinat user (straight_road_geofence.json)",
    "created": "$(Get-Date -Format 'yyyy-MM-dd')",
    "crs": "EPSG:4326",
    "travel_mode": "walking",
    "route_type": "straight",
    "buffer_meters": ${bufferM},
    "distance_meters": ${lenRound},
    "bearing_degrees": ${bearingRound},
    "gps_tolerance_note": "Buffer 20m disesuaikan untuk error tipikal NEO-6M 2-5m + drift kanopi 5-10m"
  },
  "features": [
    {
      "type": "Feature",
      "id": "origin",
      "properties": {
        "name": "Titik Asal",
        "type": "waypoint",
        "route": "Demo Kampus",
        "order": 1,
        "role": "start"
      },
      "geometry": {
        "type": "Point",
        "coordinates": [$($origin.lng), $($origin.lat)]
      }
    },
    {
      "type": "Feature",
      "id": "destination",
      "properties": {
        "name": "Titik Tujuan",
        "type": "waypoint",
        "route": "Demo Kampus",
        "order": 2,
        "role": "end"
      },
      "geometry": {
        "type": "Point",
        "coordinates": [$($dest.lng), $($dest.lat)]
      }
    },
    {
      "type": "Feature",
      "id": "route_kampus",
      "properties": {
        "name": "Jalan Lurus Kampus",
        "type": "route",
        "route": "Demo Kampus",
        "travel_mode": "walking",
        "distance_meters": ${lenRound},
        "bearing_degrees": ${bearingRound}
      },
      "geometry": {
        "type": "LineString",
        "coordinates": [
          [$($origin.lng), $($origin.lat)],
          [$($dest.lng), $($dest.lat)]
        ]
      }
    },
    {
      "type": "Feature",
      "id": "geofence_corridor",
      "properties": {
        "name": "Geofence Koridor (±${bufferM}m)",
        "type": "geofence",
        "buffer_meters": ${bufferM},
        "description": "Koridor geofence ±${bufferM}m tegak lurus dari garis jalur. Mengakomodasi error GPS NEO-6M tipikal 2-5m + margin drift 10-15m saat di bawah kanopi pohon."
      },
      "geometry": {
        "type": "Polygon",
        "coordinates": [
          [
            [$(Fmt $p1.lng), $(Fmt $p1.lat)],
            [$(Fmt $p2.lng), $(Fmt $p2.lat)],
            [$(Fmt $p3.lng), $(Fmt $p3.lat)],
            [$(Fmt $p4.lng), $(Fmt $p4.lat)],
            [$(Fmt $p1.lng), $(Fmt $p1.lat)]
          ]
        ]
      }
    }
  ]
}
"@

$json = $json -replace "`r`n", "`n"
$outPath = Join-Path (Get-Location) 'deployment/demo-branch/frontend-override/GEO.json'
[System.IO.File]::WriteAllText($outPath, $json, [System.Text.UTF8Encoding]::new($false))

# Validate
try {
    $g = Get-Content $outPath -Raw | ConvertFrom-Json
    Write-Host "JSON valid: OK"
    Write-Host "Features: $($g.features.Count)"
    Write-Host "Buffer polygon corners:"
    Write-Host ("  P1 [side A, origin]: lng=$(Fmt $p1.lng) lat=$(Fmt $p1.lat)")
    Write-Host ("  P2 [side A, dest]:   lng=$(Fmt $p2.lng) lat=$(Fmt $p2.lat)")
    Write-Host ("  P3 [side B, dest]:   lng=$(Fmt $p3.lng) lat=$(Fmt $p3.lat)")
    Write-Host ("  P4 [side B, origin]: lng=$(Fmt $p4.lng) lat=$(Fmt $p4.lat)")
    Write-Host ""
    Write-Host "Diagonal box (sanity check):"
    $boxN = [Math]::Max([Math]::Max($p1.lat, $p2.lat), [Math]::Max($p3.lat, $p4.lat))
    $boxS = [Math]::Min([Math]::Min($p1.lat, $p2.lat), [Math]::Min($p3.lat, $p4.lat))
    $boxE = [Math]::Max([Math]::Max($p1.lng, $p2.lng), [Math]::Max($p3.lng, $p4.lng))
    $boxW = [Math]::Min([Math]::Min($p1.lng, $p2.lng), [Math]::Min($p3.lng, $p4.lng))
    $widthM  = ($boxE - $boxW) * $mPerDegLng
    $heightM = ($boxN - $boxS) * $mPerDegLat
    Write-Host ("  Bounding box span: ${widthM:F1}m E-W x ${heightM:F1}m N-S")
} catch {
    throw "JSON INVALID: $_"
}

Write-Host ""
Write-Host "Written: $outPath"
