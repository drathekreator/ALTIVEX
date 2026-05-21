#!/usr/bin/env bash
# =====================================================================
# ALTIVEX Demo Publisher (mosquitto_pub simulator)
# Versi bash untuk dijalankan langsung di VM atau Linux/WSL.
# Equivalent dengan demo-publisher.ps1.
#
# Cara pakai (dari root repo ALTIVEX):
#   chmod +x scripts/demo-publisher.sh
#   ./scripts/demo-publisher.sh
#
#   # Atau dengan custom args:
#   DEVICE_ID=DEMO-CIFOR-02 LOOP_MIN=5 ./scripts/demo-publisher.sh
#
# Prerequisite: mosquitto-clients ter-install
#   sudo apt install mosquitto-clients
# =====================================================================

set -euo pipefail

# ---------------------------------------------------------------------
# Config (override via env var)
# ---------------------------------------------------------------------
DEVICE_ID="${DEVICE_ID:-DEMO-CIFOR-01}"
BROKER="${BROKER:-altivex-demo.duckdns.org}"
PORT="${PORT:-1885}"
USERNAME="${USERNAME:-altivex_demo}"
TOPIC="${TOPIC:-altivex/sensor/data}"
INTERVAL_SEC="${INTERVAL_SEC:-3}"
LOOP_MIN="${LOOP_MIN:-10}"
DRY_RUN="${DRY_RUN:-0}"

# Auto-detect password dari .env.demo kalau ada (cuma kalau kita di VM
# yang punya repo ALTIVEX). Override dengan env var ALTIVEX_DEMO_MQTT_PASSWORD.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ENV_DEMO="$REPO_ROOT/deployment/demo-branch/.env.demo"

if [[ -z "${ALTIVEX_DEMO_MQTT_PASSWORD:-}" ]] && [[ -f "$ENV_DEMO" ]]; then
    ALTIVEX_DEMO_MQTT_PASSWORD="$(grep '^MQTT_PASSWORD=' "$ENV_DEMO" | cut -d= -f2-)"
fi

if [[ -z "${ALTIVEX_DEMO_MQTT_PASSWORD:-}" ]]; then
    cat >&2 <<EOF
MQTT password belum di-set.

Pilihan:
  1. Jalankan dari root repo (auto-detect dari .env.demo):
       cd ~/ALTIVEX
       ./scripts/demo-publisher.sh

  2. Atau set manual:
       export ALTIVEX_DEMO_MQTT_PASSWORD='...'
       ./scripts/demo-publisher.sh
EOF
    exit 1
fi

# ---------------------------------------------------------------------
# Waypoints loop CIFOR-Situgede (match altivex_demo_situgede.ino)
# Format: "lng,lat" per baris
# ---------------------------------------------------------------------
WAYPOINTS=(
    "106.7518232,-6.5546282"   # Jl. CIFOR start
    "106.7510000,-6.5540000"
    "106.7498000,-6.5532000"
    "106.7482000,-6.5524000"
    "106.7469000,-6.5519000"
    "106.7457227,-6.5517073"   # Jl. Cilubang Malang
    "106.7462000,-6.5524000"
    "106.7470000,-6.5532000"
    "106.7480000,-6.5540000"
    "106.7490000,-6.5547000"
    "106.7500000,-6.5550000"
    "106.7507053,-6.5551558"   # Warung Tepi Hutan
    "106.7510000,-6.5549000"
    "106.7515000,-6.5547000"
    "106.7518232,-6.5546282"   # closed loop
)
SEG_COUNT=$((${#WAYPOINTS[@]} - 1))

# ---------------------------------------------------------------------
# Banner
# ---------------------------------------------------------------------
echo ""
echo "============================================================"
echo "ALTIVEX Demo Publisher (bash)"
echo "============================================================"
echo "Device:    $DEVICE_ID"
echo "Broker:    $BROKER:$PORT"
echo "Topic:     $TOPIC"
echo "Interval:  $INTERVAL_SEC sec"
echo "Loop:      $LOOP_MIN minutes per round"
if [[ "$DRY_RUN" == "1" ]]; then
    echo "Mode:      DRY RUN (no publish)"
else
    echo "Mode:      LIVE PUBLISH"
fi
echo "============================================================"
echo "Tekan Ctrl+C untuk stop."
echo ""

LOOP_MS=$((LOOP_MIN * 60 * 1000))
START_MS=$(($(date +%s%N) / 1000000))
COUNT=0

# ---------------------------------------------------------------------
# Helper: bc-based interpolation (awk lebih portable di VM minimal)
# ---------------------------------------------------------------------
interpolate() {
    local progress="$1"
    awk -v p="$progress" -v segs="$SEG_COUNT" \
        -v "$(printf 'wp_%d=%s ' $(seq 0 $((${#WAYPOINTS[@]}-1))) "${WAYPOINTS[@]}")" '
        BEGIN {
            # Note: dynamic var array via -v is messy in awk; use simple loop instead
        }
    ' </dev/null 2>/dev/null
    # Fallback: pure bash + awk for the math
}

# Simpler approach: gunakan awk inline tiap iterasi
get_position() {
    local progress="$1"
    awk -v p="$progress" -v segs="$SEG_COUNT" -v wps="${WAYPOINTS[*]}" '
    BEGIN {
        n = split(wps, arr, " ")
        seg_frac = p * segs
        seg_idx = int(seg_frac)
        if (seg_idx >= segs) seg_idx = segs - 1
        t = seg_frac - seg_idx

        split(arr[seg_idx + 1], a, ",")
        split(arr[seg_idx + 2], b, ",")

        lng = a[1] + (b[1] - a[1]) * t
        lat = a[2] + (b[2] - a[2]) * t
        printf "%.6f %.6f", lng, lat
    }'
}

# ---------------------------------------------------------------------
# Main loop
# ---------------------------------------------------------------------
while true; do
    NOW_MS=$(($(date +%s%N) / 1000000))
    ELAPSED=$((NOW_MS - START_MS))
    PROGRESS=$(awk -v e="$ELAPSED" -v l="$LOOP_MS" 'BEGIN { printf "%.6f", (e % l) / l }')

    # Position
    read -r LNG LAT <<< "$(get_position "$PROGRESS")"

    # Battery decay: 100 -> 20, drop 1% per 30 sec
    DROP=$((ELAPSED / 30000))
    BATTERY=$((100 - DROP))
    if [[ $BATTERY -lt 20 ]]; then BATTERY=20; fi

    # Build JSON
    PAYLOAD=$(printf '{"id_perangkat":"%s","latitude":%s,"longitude":%s,"battery":%d}' \
        "$DEVICE_ID" "$LAT" "$LNG" "$BATTERY")

    COUNT=$((COUNT + 1))
    STAMP=$(date +%H:%M:%S)
    PROGRESS_PCT=$(awk -v p="$PROGRESS" 'BEGIN { printf "%5.1f", p * 100 }')

    printf "[%s] #%-4d loop=%s%% bat=%3d%% -> %s\n" \
        "$STAMP" "$COUNT" "$PROGRESS_PCT" "$BATTERY" "$PAYLOAD"

    if [[ "$DRY_RUN" != "1" ]]; then
        if ! mosquitto_pub \
            -h "$BROKER" -p "$PORT" \
            -u "$USERNAME" -P "$ALTIVEX_DEMO_MQTT_PASSWORD" \
            -t "$TOPIC" \
            -q 1 \
            -m "$PAYLOAD" 2>/dev/null
        then
            echo "  ⚠️  mosquitto_pub gagal (exit $?)" >&2
        fi
    fi

    sleep "$INTERVAL_SEC"
done
