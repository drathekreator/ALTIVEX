# Tarik POI Gunung Gede-Pangrango dari OpenStreetMap via Overpass API.
# Output: deployment/pangrango-pois.json (raw OSM) + cocokkan ke
# legenda PDF (peak, shelter, hot_spring, telaga, alun-alun, dll.)

$ErrorActionPreference = 'Stop'

# Bounding box approximate kawasan Cibodas-Gede-Pangrango
# (south, west, north, east)
$bbox = "-6.84,106.95,-6.74,107.02"

$query = @"
[out:json][timeout:60];
(
  node["natural"="peak"]($bbox);
  node["natural"="hot_spring"]($bbox);
  node["natural"="waterfall"]($bbox);
  node["natural"="water"]($bbox);
  node["natural"="volcano"]($bbox);
  node["tourism"="alpine_hut"]($bbox);
  node["tourism"="camp_site"]($bbox);
  node["tourism"="wilderness_hut"]($bbox);
  node["tourism"="information"]($bbox);
  node["tourism"="viewpoint"]($bbox);
  node["tourism"="picnic_site"]($bbox);
  node["amenity"="shelter"]($bbox);
  node["amenity"="ranger_station"]($bbox);
  node["amenity"="parking"]($bbox);
  node["amenity"="drinking_water"]($bbox);
  node["amenity"="toilets"]($bbox);
  node["mountain_pass"="yes"]($bbox);
  way["natural"="water"]($bbox);
  way["natural"="hot_spring"]($bbox);
  way["natural"="waterfall"]($bbox);
  way["leisure"="park"]($bbox);
);
out center;
"@

Write-Host "Querying Overpass API..."
$encodedBody = "data=" + [uri]::EscapeDataString($query)
$headers = @{
    'User-Agent' = 'altivex-trail-mapper/1.0 (kontak: indra@altivex)'
    'Accept'     = 'application/json'
}
$resp = Invoke-RestMethod -Uri 'https://overpass-api.de/api/interpreter' `
    -Method Post -Body $encodedBody `
    -ContentType 'application/x-www-form-urlencoded' `
    -Headers $headers `
    -TimeoutSec 90

$count = $resp.elements.Count
Write-Host "Got $count elements"

# Kategorikan + sort
$pois = @()
foreach ($e in $resp.elements) {
    $tags = $e.tags
    if (-not $tags) { continue }

    $kind = ""
    if ($tags.natural) { $kind = "natural=$($tags.natural)" }
    elseif ($tags.tourism) { $kind = "tourism=$($tags.tourism)" }
    elseif ($tags.amenity) { $kind = "amenity=$($tags.amenity)" }
    elseif ($tags.mountain_pass) { $kind = "mountain_pass" }
    elseif ($tags.leisure) { $kind = "leisure=$($tags.leisure)" }

    $lat = if ($e.lat) { $e.lat } else { $e.center.lat }
    $lon = if ($e.lon) { $e.lon } else { $e.center.lon }
    if (-not $lat -or -not $lon) { continue }

    $pois += [PSCustomObject]@{
        kind        = $kind
        name        = if ($tags.name) { $tags.name } else { "<unnamed>" }
        elevation   = $tags.ele
        lat         = [double]$lat
        lon         = [double]$lon
        type        = if ($tags.tourism) { "tourism" } elseif ($tags.natural) { "natural" } else { "amenity" }
    }
}

# Save raw to file
$rawPath = Join-Path (Get-Location) 'deployment/pangrango-pois.json'
$pois | ConvertTo-Json -Depth 5 | Set-Content -Path $rawPath -Encoding utf8
Write-Host "Raw POI list saved to $rawPath ($($pois.Count) items)"

# Print summary by kind
Write-Host ""
Write-Host "Summary by kind:"
$pois | Group-Object kind | Sort-Object Count -Descending | ForEach-Object {
    Write-Host ("  {0,-30} {1}" -f $_.Name, $_.Count)
}

# Print named POIs sorted by kind
Write-Host ""
Write-Host "Named POIs:"
$pois | Where-Object { $_.name -ne "<unnamed>" } |
    Sort-Object kind, name |
    ForEach-Object {
        $ele = if ($_.elevation) { " ($($_.elevation)m)" } else { "" }
        Write-Host ("  {0,-25} {1}{2} -> [{3:F5}, {4:F5}]" -f $_.kind, $_.name, $ele, $_.lon, $_.lat)
    }
