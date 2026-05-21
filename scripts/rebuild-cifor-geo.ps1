# Rebuild GEO.json untuk demo altivex-demo.duckdns.org dengan rute walking
# loop CIFOR -> Jl. CIFOR -> Situ Gede -> kembali ke CIFOR.
#
# Pakai OSRM foot profile (routing.openstreetmap.de) supaya polyline
# benar-benar ngikut jalur pejalan kaki di OSM, bukan vektor lurus.
#
# Output: deployment/demo-branch/frontend-override/GEO.json

param(
    [string]$Output = 'deployment/demo-branch/frontend-override/GEO.json'
)

$ErrorActionPreference = 'Stop'

# Loop waypoints. Urutan match screenshot Google Maps user:
#   1. CIFOR pintu masuk (Jl. CIFOR)
#   2. Sisi utara Situ Gede (lewat Penangkaran Rusa)
#   3. Sisi barat Situ Gede
#   4. Sisi selatan Situ Gede
#   5. Balik ke CIFOR via Jl. CIFOR
$waypointsLng = @(
    @{ lng = 106.7506; lat = -6.5566; name = 'CIFOR (Center for International Forestry Research)';    role = 'origin'      },
    @{ lng = 106.7493; lat = -6.5500; name = 'Sisi Utara Situ Gede (Penangkaran Rusa)';                role = 'midpoint'    },
    @{ lng = 106.7440; lat = -6.5523; name = 'Sisi Barat Situ Gede';                                   role = 'midpoint'    },
    @{ lng = 106.7460; lat = -6.5570; name = 'Sisi Selatan Situ Gede';                                 role = 'midpoint'    },
    @{ lng = 106.7506; lat = -6.5566; name = 'CIFOR (kembali)';                                        role = 'destination' }
)

$wpStr = ($waypointsLng | ForEach-Object { "$($_.lng),$($_.lat)" }) -join ';'
$url = "https://routing.openstreetmap.de/routed-foot/route/v1/walking/$wpStr" +
       '?overview=full&geometries=geojson&steps=false'

Write-Host "Fetching OSRM foot route..."
Write-Host "  $url"
Write-Host ""

$resp = Invoke-RestMethod -Uri $url -Method Get -TimeoutSec 30
if ($resp.code -ne 'Ok') {
    throw "OSRM returned code=$($resp.code) message=$($resp.message)"
}

$route       = $resp.routes[0]
$coords      = $route.geometry.coordinates
$distanceKm  = [math]::Round($route.distance / 1000, 2)
$durationMin = [math]::Round($route.duration / 60, 1)

Write-Host "OSRM result:"
Write-Host "  Points:   $($coords.Count)"
Write-Host "  Distance: $distanceKm km"
Write-Host "  Duration: $durationMin min (walking)"
Write-Host ""

# Build GeoJSON FeatureCollection
$features = New-Object System.Collections.ArrayList

# Origin + Destination + intermediate waypoints sebagai Point
$order = 1
foreach ($wp in $waypointsLng) {
    [void]$features.Add([ordered]@{
        type       = 'Feature'
        id         = "waypoint-$order"
        properties = [ordered]@{
            name  = $wp.name
            type  = 'waypoint'
            route = 'Jl. CIFOR Loop'
            order = $order
            role  = $wp.role
        }
        geometry = [ordered]@{
            type        = 'Point'
            coordinates = @([double]$wp.lng, [double]$wp.lat)
        }
    })
    $order++
}

# Route LineString
[void]$features.Add([ordered]@{
    type       = 'Feature'
    id         = 'route-cifor-situgede'
    properties = [ordered]@{
        name         = 'Loop CIFOR - Situ Gede - CIFOR'
        type         = 'route'
        route        = 'Jl. CIFOR Loop'
        from         = 'CIFOR'
        to           = 'CIFOR (via Situ Gede)'
        distance_km  = $distanceKm
        duration_min = $durationMin
        travel_mode  = 'walking'
    }
    geometry = [ordered]@{
        type        = 'LineString'
        coordinates = $coords
    }
})

$geojson = [ordered]@{
    type     = 'FeatureCollection'
    name     = 'Geofence - CIFOR loop (Bogor)'
    metadata = [ordered]@{
        description   = 'Walking loop CIFOR -> Situ Gede -> CIFOR via OSRM foot profile'
        source        = 'routing.openstreetmap.de (OSRM foot)'
        created       = (Get-Date -Format 'yyyy-MM-dd')
        crs           = 'EPSG:4326'
        distance_km   = $distanceKm
        duration_min  = $durationMin
        point_count   = $coords.Count
    }
    features = $features.ToArray()
}

$json = $geojson | ConvertTo-Json -Depth 99 -Compress:$false
$json = $json -replace "`r`n", "`n"

$dir = Split-Path -Parent $Output
if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
$outAbs = Join-Path (Get-Location) $Output

[System.IO.File]::WriteAllText(
    $outAbs,
    $json,
    [System.Text.UTF8Encoding]::new($false)
)

Write-Host "Written: $Output ($($coords.Count) points, $([math]::Round((Get-Item $outAbs).Length / 1024, 1)) KB)"
