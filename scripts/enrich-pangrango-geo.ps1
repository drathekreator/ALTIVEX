# Enrich frontend/GEO.json dengan waypoint POI dari OpenStreetMap
# (peak, camp_site, telaga, pos shelter) plus beberapa koordinat
# well-known yang tidak ada di OSM (Cibodas trailhead, Cibeureum
# waterfall, Air Panas).
#
# Source data:
#   1. OSM via Overpass API — node bertag natural/tourism/amenity
#      di bbox kawasan TNGGP
#   2. Manual koordinat untuk POI penting yang tidak ada di OSM
#      (di-cross-reference ke Google Maps / peta TNGGP resmi)
#
# LineString existing di GEO.json TIDAK diubah. Hanya Point waypoint
# yang ditambah.

$ErrorActionPreference = 'Stop'

$bbox = "-6.84,106.95,-6.74,107.02"

# 1. Query Overpass API
$query = @"
[out:json][timeout:60];
(
  node["natural"="peak"]($bbox);
  node["natural"="volcano"]($bbox);
  node["tourism"="camp_site"]($bbox);
  node["place"="locality"]($bbox);
);
out;
"@
$encodedBody = "data=" + [uri]::EscapeDataString($query)
$headers = @{
    'User-Agent' = 'altivex-trail-mapper/1.0'
    'Accept'     = 'application/json'
}

Write-Host "Querying Overpass API..."
$resp = Invoke-RestMethod -Uri 'https://overpass-api.de/api/interpreter' `
    -Method Post -Body $encodedBody `
    -ContentType 'application/x-www-form-urlencoded' `
    -Headers $headers -TimeoutSec 90
Write-Host "Got $($resp.elements.Count) elements from OSM"

# 2. Filter + dedupe (place=locality dan tourism=camp_site sering
#    duplikat untuk lokasi yang sama, pakai prioritas tourism)
$pois = @{}
foreach ($e in $resp.elements) {
    $t = $e.tags
    if (-not $t -or -not $t.name) { continue }
    $name = $t.name
    $key = $name.ToLower()

    # Tentukan kind + icon
    $kind = ""
    $iconHint = ""
    if ($t.natural -eq "peak" -or $t.natural -eq "volcano") {
        $kind = "Puncak"
        $iconHint = "summit"
    } elseif ($t.tourism -eq "camp_site") {
        if ($name -match "Telaga|Lake|Danau") { $kind = "Telaga"; $iconHint = "water" }
        elseif ($name -match "Alun|Mandalawangi") { $kind = "Alun-Alun"; $iconHint = "camp" }
        else { $kind = "Pos / Shelter"; $iconHint = "shelter" }
    } elseif ($t.place -eq "locality") {
        # Skip kalau sudah ada dari camp_site
        if ($pois.ContainsKey($key)) { continue }
        $kind = "Pos / Shelter"
        $iconHint = "shelter"
    } else {
        continue
    }

    $pois[$key] = [PSCustomObject]@{
        name       = $name
        kind       = $kind
        elevation  = if ($t.ele) { [int]$t.ele } else { $null }
        lat        = [double]$e.lat
        lon        = [double]$e.lon
        icon       = $iconHint
        source     = "osm"
    }
}
Write-Host "Filtered to $($pois.Count) unique named POIs"

# 3. Tambah POI well-known yang tidak ada di OSM. Koordinat
#    di-cross-reference ke peta TNGGP + Google Maps satellite view.
#    Source: peta-jalur-pendakian-gunung-gede-pangrango.pdf,
#    Google Maps query "Pintu Gerbang Cibodas", "Curug Cibeureum",
#    "Air Panas Gunung Gede".
$manualPois = @(
    [PSCustomObject]@{
        name="Pintu Gerbang Cibodas"; kind="Registrasi"
        elevation=1341; lat=-6.74500; lon=106.99170
        icon="gate"; source="manual:google-maps"
    }
    [PSCustomObject]@{
        name="Pintu Gerbang Gunung Putri"; kind="Registrasi"
        elevation=1500; lat=-6.75817; lon=107.00780
        icon="gate"; source="manual:google-maps"
    }
    [PSCustomObject]@{
        name="Pintu Gerbang Selabintana"; kind="Registrasi"
        elevation=1000; lat=-6.84712; lon=106.96132
        icon="gate"; source="manual:geo.json-start"
    }
    [PSCustomObject]@{
        name="Curug Cibeureum"; kind="Curug"
        elevation=1670; lat=-6.76630; lon=106.98520
        icon="waterfall"; source="manual:google-maps"
    }
    [PSCustomObject]@{
        name="Air Panas (Hot Spring)"; kind="Air Panas"
        elevation=2150; lat=-6.77280; lon=106.97650
        icon="hotspring"; source="manual:trail-guide"
    }
    [PSCustomObject]@{
        name="Surya Kencana"; kind="Padang Edelweis"
        elevation=2750; lat=-6.78089; lon=106.98917
        icon="flower"; source="manual:trail-guide"
    }
    [PSCustomObject]@{
        name="Tanjakan Setan"; kind="Tanjakan"
        elevation=2480; lat=-6.78417; lon=106.98556
        icon="climb"; source="manual:trail-guide"
    }
)
foreach ($p in $manualPois) {
    $key = $p.name.ToLower()
    $pois[$key] = $p
}
Write-Host "Total POIs after manual additions: $($pois.Count)"

# 4. Load existing GEO.json
$geoPath = Join-Path (Get-Location) 'frontend/GEO.json'
$geo = Get-Content $geoPath -Raw | ConvertFrom-Json
$existingFeatures = @($geo.features)
Write-Host "Existing GEO.json: $($existingFeatures.Count) features"

# 5. Build Point features dari POIs. Skip kalau sudah ada Point
#    dengan nama match di GEO.json (idempotent — bisa run ulang).
$existingNames = @{}
foreach ($f in $existingFeatures) {
    if ($f.geometry.type -eq "Point" -and $f.properties.name) {
        $existingNames[$f.properties.name.ToLower()] = $true
    }
}

$newPointFeatures = @()
foreach ($key in $pois.Keys) {
    if ($existingNames.ContainsKey($key)) { continue }
    $p = $pois[$key]
    $props = [ordered]@{
        name    = $p.name
        type    = $p.kind
        icon    = $p.icon
        source  = $p.source
    }
    if ($p.elevation) { $props["elevation_m"] = $p.elevation }

    $newPointFeatures += [ordered]@{
        type       = "Feature"
        properties = $props
        geometry   = [ordered]@{
            type        = "Point"
            coordinates = @($p.lon, $p.lat)
        }
    }
}
Write-Host "New Point features to add: $($newPointFeatures.Count)"

# 6. Append + write
$allFeatures = $existingFeatures + $newPointFeatures
$out = [ordered]@{
    type     = "FeatureCollection"
    features = $allFeatures
}

$json = $out | ConvertTo-Json -Depth 99
# Force LF untuk konsistensi dengan .gitattributes
$json = $json -replace "`r`n", "`n"
[System.IO.File]::WriteAllText($geoPath, $json, [System.Text.UTF8Encoding]::new($false))

Write-Host ""
Write-Host "Updated $geoPath"
Write-Host "Total features now: $($allFeatures.Count)"

# 7. Validate
try {
    $null = Get-Content $geoPath -Raw | ConvertFrom-Json
    Write-Host "JSON valid: OK"
} catch {
    throw "JSON INVALID after write: $_"
}

# 8. Print summary
Write-Host ""
Write-Host "Summary by feature type:"
$allFeatures | Group-Object { $_.geometry.type } | ForEach-Object {
    Write-Host ("  {0,-12} {1}" -f $_.Name, $_.Count)
}

Write-Host ""
Write-Host "New POIs ditambahkan:"
$newPointFeatures | ForEach-Object {
    $ele = if ($_.properties["elevation_m"]) { " ($($_.properties["elevation_m"])m)" } else { "" }
    Write-Host ("  + {0,-22} {1}{2}" -f $_.properties.type, $_.properties.name, $ele)
}
