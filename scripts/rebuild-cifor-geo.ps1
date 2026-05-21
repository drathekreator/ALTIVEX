# Rebuild GEO.json untuk demo altivex-demo.duckdns.org dengan rute cycling
# loop CIFOR -> Cilubang Malang -> Warung Tepi Hutan -> CIFOR.
#
# Strategi: 3 leg terpisah via OSRM bicycle profile, lalu di-join.
# Hasilnya jalur ngikutin jalan beraspal (cycling profile menghindari
# footway hutan, tapi izinkan jalan kampung kecil yang bisa dilewati
# sepeda — match sama referensi user yang travel_mode cycling).
#
# Waypoints sesuai referensi user (situgede_loop_geofence.json):
#   1. Jalan CIFOR (Start/Finish): 106.7518232, -6.5546282
#   2. Jl. Cilubang Malang No.37:  106.7457227, -6.5517073
#   3. Warung Tepi Hutan:          106.7507053, -6.5551558
#
# Output: deployment/demo-branch/frontend-override/GEO.json

param(
    [string]$Output = 'deployment/demo-branch/frontend-override/GEO.json'
)

$ErrorActionPreference = 'Stop'

$wp1 = @{ lng = 106.7518232; lat = -6.5546282; name = 'Jalan CIFOR (Start / Finish)';  role = 'start_finish' }
$wp2 = @{ lng = 106.7457227; lat = -6.5517073; name = 'Jl. Cilubang Malang No.37';      role = 'via'          }
$wp3 = @{ lng = 106.7507053; lat = -6.5551558; name = 'Warung Tepi Hutan';              role = 'via'          }

function Get-Leg($from, $to, $label) {
    $url = "https://routing.openstreetmap.de/routed-bike/route/v1/cycling/$($from.lng),$($from.lat);$($to.lng),$($to.lat)?overview=full&geometries=geojson"
    Write-Host "  Fetching $label..."
    $r = (Invoke-RestMethod -Uri $url -TimeoutSec 30).routes[0]
    Write-Host "    -> $([math]::Round($r.distance/1000,2)) km, $($r.geometry.coordinates.Count) points"
    return $r.geometry.coordinates
}

Write-Host "OSRM bicycle profile, 3 legs:"
$leg1 = Get-Leg $wp1 $wp2 "Leg 1 ($($wp1.name) -> $($wp2.name))"
$leg2 = Get-Leg $wp2 $wp3 "Leg 2 ($($wp2.name) -> $($wp3.name))"
$leg3 = Get-Leg $wp3 $wp1 "Leg 3 ($($wp3.name) -> $($wp1.name))"

# Join: skip first point of leg 2 dan leg 3 (duplikat dari leg sebelumnya)
$all = New-Object System.Collections.ArrayList
foreach ($p in $leg1) { [void]$all.Add(@($p[0], $p[1])) }
for ($i = 1; $i -lt $leg2.Count; $i++) { [void]$all.Add(@($leg2[$i][0], $leg2[$i][1])) }
for ($i = 1; $i -lt $leg3.Count; $i++) { [void]$all.Add(@($leg3[$i][0], $leg3[$i][1])) }

# Compute total distance via Haversine
$totalKm = 0
for ($i = 1; $i -lt $all.Count; $i++) {
    $lat1 = $all[$i-1][1] * [math]::PI / 180
    $lat2 = $all[$i][1] * [math]::PI / 180
    $dLat = $lat2 - $lat1
    $dLng = ($all[$i][0] - $all[$i-1][0]) * [math]::PI / 180
    $a = [math]::Sin($dLat/2)*[math]::Sin($dLat/2) + [math]::Cos($lat1)*[math]::Cos($lat2)*[math]::Sin($dLng/2)*[math]::Sin($dLng/2)
    $totalKm += 2 * [math]::Asin([math]::Sqrt($a)) * 6371
}
$totalKmRounded = [math]::Round($totalKm, 2)

Write-Host ""
Write-Host "Joined: $($all.Count) points, $totalKmRounded km"

# Build coords block as JSON string
$coordPairs = $all | ForEach-Object { "[$($_[0]),$($_[1])]" }
$coordsBlock = ($coordPairs -join ",`n      ")

$json = @"
{
  "type": "FeatureCollection",
  "name": "Geofence - Loop Bersepeda Situgede",
  "metadata": {
    "description": "Loop bersepeda CIFOR -> Cilubang Malang -> Warung Tepi Hutan -> CIFOR",
    "source": "OSRM bicycle profile (routing.openstreetmap.de)",
    "created": "$(Get-Date -Format 'yyyy-MM-dd')",
    "crs": "EPSG:4326",
    "travel_mode": "cycling",
    "route_type": "loop",
    "distance_km": $totalKmRounded,
    "point_count": $($all.Count)
  },
  "features": [
    {
      "type": "Feature",
      "id": "waypoint_1",
      "properties": {
        "name": "$($wp1.name)",
        "type": "waypoint",
        "route": "Jl. CIFOR Loop",
        "order": 1,
        "role": "$($wp1.role)"
      },
      "geometry": { "type": "Point", "coordinates": [$($wp1.lng), $($wp1.lat)] }
    },
    {
      "type": "Feature",
      "id": "waypoint_2",
      "properties": {
        "name": "$($wp2.name)",
        "type": "waypoint",
        "route": "Jl. CIFOR Loop",
        "order": 2,
        "role": "$($wp2.role)"
      },
      "geometry": { "type": "Point", "coordinates": [$($wp2.lng), $($wp2.lat)] }
    },
    {
      "type": "Feature",
      "id": "waypoint_3",
      "properties": {
        "name": "$($wp3.name)",
        "type": "waypoint",
        "route": "Jl. CIFOR Loop",
        "order": 3,
        "role": "$($wp3.role)"
      },
      "geometry": { "type": "Point", "coordinates": [$($wp3.lng), $($wp3.lat)] }
    },
    {
      "type": "Feature",
      "id": "route_loop",
      "properties": {
        "name": "Loop Bersepeda Situgede",
        "type": "route",
        "route": "Jl. CIFOR Loop",
        "travel_mode": "cycling",
        "distance_km": $totalKmRounded
      },
      "geometry": {
        "type": "LineString",
        "coordinates": [
      $coordsBlock
        ]
      }
    }
  ]
}
"@

$json = $json -replace "`r`n", "`n"
$dir = Split-Path -Parent $Output
if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
$outAbs = Join-Path (Get-Location) $Output

[System.IO.File]::WriteAllText(
    $outAbs,
    $json,
    [System.Text.UTF8Encoding]::new($false)
)

Write-Host ""
Write-Host "Written: $Output ($([math]::Round((Get-Item $outAbs).Length/1024,1)) KB)"

# Validate
try { $null = Get-Content $outAbs -Raw | ConvertFrom-Json; Write-Host "JSON valid: OK" } catch { throw "JSON INVALID: $_" }
