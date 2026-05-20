# Rebuild puncak_geofence.json + demo-branch GEO.json dari OSRM (snap-to-road).
# Output: jalur LineString yang persis ngikutin Jl. Raya Puncak (Ciawi -> Gadog -> Cisarua),
# bukan vektor lurus diagonal.
#
# Pakai endpoint OSRM publik. Kalau rate-limit, ganti ke instance self-hosted.

param(
    [string]$Output1 = 'frontend/puncak_geofence.json',
    [string]$Output2 = 'deployment/demo-branch/frontend-override/GEO.json'
)

$ErrorActionPreference = 'Stop'

# Origin (Ciawi) -> Destination (Cisarua). Sama seperti pin Google Maps di screenshot.
$origin      = '106.8589743,-6.6552671'
$destination = '106.9461913,-6.6853882'

$url = "https://router.project-osrm.org/route/v1/driving/$origin;$destination" +
       '?overview=full&geometries=geojson&steps=false'

Write-Host "Fetching $url ..."
$resp = Invoke-RestMethod -Uri $url -Method Get -TimeoutSec 30

if ($resp.code -ne 'Ok') {
    throw "OSRM returned code=$($resp.code)"
}

$route        = $resp.routes[0]
$coords       = $route.geometry.coordinates
$distanceKm   = [math]::Round($route.distance / 1000, 2)
$durationMin  = [math]::Round($route.duration / 60, 1)

Write-Host "Got $($coords.Count) points, distance=$distanceKm km, duration=$durationMin min"

# Build GeoJSON FeatureCollection
$geojson = [ordered]@{
    type     = 'FeatureCollection'
    name     = 'Geofence - Jl. Raya Puncak (Ciawi -> Gadog/Cisarua)'
    metadata = [ordered]@{
        description    = 'Snap-to-road via OSRM (router.project-osrm.org)'
        source         = 'OSRM driving profile'
        created        = (Get-Date -Format 'yyyy-MM-dd')
        crs            = 'EPSG:4326'
        distance_km    = $distanceKm
        duration_min   = $durationMin
        point_count    = $coords.Count
    }
    features = @(
        [ordered]@{
            type       = 'Feature'
            id         = 'origin'
            properties = [ordered]@{
                name  = 'Titik Asal - Ciawi'
                type  = 'waypoint'
                route = 'Jl. Raya Puncak'
                order = 1
            }
            geometry = [ordered]@{
                type        = 'Point'
                coordinates = @([double]106.8589743, [double]-6.6552671)
            }
        },
        [ordered]@{
            type       = 'Feature'
            id         = 'destination'
            properties = [ordered]@{
                name  = 'Titik Tujuan - Cisarua'
                type  = 'waypoint'
                route = 'Jl. Raya Puncak'
                order = 2
            }
            geometry = [ordered]@{
                type        = 'Point'
                coordinates = @([double]106.9461913, [double]-6.6853882)
            }
        },
        [ordered]@{
            type       = 'Feature'
            id         = 'route'
            properties = [ordered]@{
                name         = 'Jalur Jl. Raya Puncak (snap-to-road)'
                type         = 'route'
                route        = 'Jl. Raya Puncak'
                road         = 'Jl. Raya Puncak'
                from         = 'Ciawi'
                to           = 'Cisarua'
                distance_km  = $distanceKm
                travel_mode  = 'driving'
            }
            geometry = [ordered]@{
                type        = 'LineString'
                coordinates = $coords
            }
        }
    )
}

$json = $geojson | ConvertTo-Json -Depth 99 -Compress:$false

# Convert CRLF -> LF supaya konsisten dengan .gitattributes
$json = $json -replace "`r`n", "`n"

# Resolve absolute path untuk Output1 (file mungkin sudah ada atau belum)
$out1Abs = if (Test-Path $Output1) { (Resolve-Path $Output1).Path } else { Join-Path (Get-Location) $Output1 }

# Write dengan UTF-8 tanpa BOM, eol=LF
[System.IO.File]::WriteAllText(
    $out1Abs,
    $json,
    [System.Text.UTF8Encoding]::new($false)
)

# Pastikan parent dir Output2 ada, lalu tulis
$dir2 = Split-Path -Parent $Output2
if (-not (Test-Path $dir2)) { New-Item -ItemType Directory -Force -Path $dir2 | Out-Null }
[System.IO.File]::WriteAllText(
    (Join-Path (Get-Location) $Output2),
    $json,
    [System.Text.UTF8Encoding]::new($false)
)

Write-Host "Written:"
Write-Host "  $Output1 ($($coords.Count) points)"
Write-Host "  $Output2 ($($coords.Count) points)"
