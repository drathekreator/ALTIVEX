<div align="center">

# ⛰️ ALTIVEX

**Sistem pemantauan pendaki real-time untuk Taman Nasional Gunung Gede Pangrango**

GPS tracking lewat ESP32 + LoRa/MQTT, geofence buffer otomatis pada jalur resmi (Cibodas / Gn Putri / Selabintana), peringatan keluar jalur, dan dashboard live di basecamp.

[![Rust](https://img.shields.io/badge/Rust-1.90-orange?logo=rust)](https://www.rust-lang.org/)
[![Actix](https://img.shields.io/badge/Actix--Web-4-blue)](https://actix.rs/)
[![Postgres](https://img.shields.io/badge/Postgres-15-336791?logo=postgresql&logoColor=white)](https://www.postgresql.org/)
[![MQTT](https://img.shields.io/badge/MQTT-Mosquitto%202-660066)](https://mosquitto.org/)
[![License](https://img.shields.io/badge/License-MIT-green)](LICENSE)
[![Production](https://img.shields.io/badge/Status-Production-success)]()

[Demo](https://altivex-pangrango.duckdns.org) · [Dokumentasi Deploy](deployment/README.md) · [Brief Multi-Project](deployment/multi-project-host/AGENT_BRIEF.md)

</div>

---

## ✨ Fitur

- 📡 **Real-time tracking** — koordinat dari ESP32 → MQTT → Postgres → WebSocket → dashboard, latency sub-detik
- 🗺️ **Geofence otomatis** pada jalur resmi (Turf.js buffer 50 m), alert in-app + browser notification saat keluar koridor
- 👥 **Manajemen pendaki** — registrasi, pencarian, riwayat per pendaki, ekspor CSV
- 🚨 **Downlink peringatan** — basecamp bisa kirim sinyal getar ke device pendaki via Serial → LoRa
- 🔐 **Production-grade security** — auth token Bearer untuk endpoint mutating, MQTT auth wajib, payload validation, JSON injection guard
- 🧪 **Property-based testing** — 11 PBT Rust + 8 vitest frontend, semuanya hijau
- 🌗 **Modern Warm + dark mode** — neobrutalism aesthetic, persisten via localStorage
- 🛰️ **Resilient connectivity** — reconnect MQTT exponential backoff, WS fallback polling, idempotent INSERT untuk QoS=1

## 🏗️ Arsitektur

```
            ┌──────────────────────────────────────┐
            │  Pendaki di Pangrango (ESP32)        │
            │  GPS NEO-6M + LoRa / WiFi → MQTT     │
            └────────────────┬─────────────────────┘
                             │ publish altivex/sensor/data
                             ▼
        ┌────────────────────────────────────────────┐
        │  Cloud (GCP VM, asia-southeast2-a)         │
        │  ┌──────────────┐                          │
        │  │ Mosquitto 2  │  auth required           │
        │  └──────┬───────┘                          │
        │         │ rumqttc subscribe (QoS=1)        │
        │         ▼                                  │
        │  ┌──────────────┐    ┌──────────────────┐  │
        │  │ Backend Rust │───▶│ Postgres 15       │  │
        │  │ Actix-Web 4  │    │ log_sensor +     │  │
        │  │              │    │ pendaki          │  │
        │  └──────┬───────┘    └──────────────────┘  │
        │         │ broadcast via WebSocket          │
        └─────────┼──────────────────────────────────┘
                  │ wss:// (TLS via nginx)
                  ▼
        ┌──────────────────────────────────────────┐
        │ Dashboard Basecamp                       │
        │ Leaflet + Turf.js + WebSocket            │
        │ alert banner • geofence buffer • export  │
        └──────────────────────────────────────────┘
```

Stack:

| Layer       | Tech                                              |
|-------------|---------------------------------------------------|
| Backend     | Rust 1.90, Actix-Web 4, Tokio, sqlx 0.7           |
| Broker      | Mosquitto 2 (auth + persistence)                  |
| Database    | PostgreSQL 15 (UNIQUE INDEX dedupe untuk QoS=1)   |
| Frontend    | Vanilla JS + Leaflet 1.9 + Turf.js 6 + WebSocket  |
| Reverse proxy | nginx + Let's Encrypt                           |
| Container   | Docker Compose v2 (multi-stage build, non-root)   |
| Device      | ESP32 + NEO-6M GPS + vibrator (template provided) |

## 🚀 Quickstart (development lokal)

Prasyarat: Docker Desktop, Rust 1.90+ (untuk run native), `mosquitto_pub` (testing).

```bash
# 1. Clone
git clone https://github.com/drathekreator/ALTIVEX.git
cd ALTIVEX

# 2. Generate config + secret
cp .env.example .env
# edit .env, isi POSTGRES_*, MQTT_*, API_AUTH_TOKEN
# (atau pakai bootstrap.sh untuk generate otomatis — lihat deployment/)

# 3. Generate mosquitto password
docker run --rm -v "$PWD/mosquitto/config":/work \
    -w /work eclipse-mosquitto:latest \
    mosquitto_passwd -b -c passwd "$(grep ^MQTT_USERNAME .env | cut -d= -f2)" \
                                  "$(grep ^MQTT_PASSWORD .env | cut -d= -f2)"

# 4. Up
docker compose up -d --build

# 5. Cek
docker compose logs -f backend
# yang harus muncul:
# 🚀 Server ALTIVEX berjalan di http://0.0.0.0:8080
# 📡 MQTT Subscriber aktif di topic: altivex/sensor/data (QoS=AtLeastOnce)
```

Buka `http://localhost:8080` → masukkan API token saat di-prompt → jadi.

### Test publish manual

```bash
docker compose exec mosquitto mosquitto_pub \
  -h 127.0.0.1 -p 1883 \
  -u "$(grep ^MQTT_USERNAME .env | cut -d= -f2)" \
  -P "$(grep ^MQTT_PASSWORD .env | cut -d= -f2)" \
  -t altivex/sensor/data -q 1 \
  -m '{"id_perangkat":"ALAT-001","latitude":-6.7711,"longitude":106.96}'
```

Yang harus muncul di backend log:
```
📥 MQTT publish diterima: id=ALAT-001 lat=-6.7711 lon=106.96
💾 Insert OK ke log_sensor: id=ALAT-001 (1 row).
📣 WS broadcast → 1 subscriber.
```

## 🌍 Deployment ke production

Ada runbook lengkap untuk deploy ke GCP VM dengan domain DuckDNS + nginx + Let's Encrypt:

📖 **[`deployment/README.md`](deployment/README.md)** — runbook end-to-end

Highlight:
- `bootstrap.sh` — idempotent generator `.env` + mosquitto passwd (hex-only password supaya URL-safe di `DATABASE_URL`)
- `Caddyfile` — alternatif Caddy reverse proxy (auto TLS)
- `docker-compose.prod.yml` — overlay tambahan untuk production
- `esp32-templates/` — `.ino` siap-pakai untuk plaintext (port 1883) dan TLS (port 8883) MQTT publish

### Mau deploy project lain di VM yang sama?

📖 **[`deployment/multi-project-host/AGENT_BRIEF.md`](deployment/multi-project-host/AGENT_BRIEF.md)**

Brief untuk AI agent project kedua: konvensi naming wajib, dua strategi MQTT (shared vs isolated broker), checklist verifikasi pre-deploy, dan red lines anti-collision.

## 🧪 Testing

PBT (property-based testing) Rust:
```bash
cargo test --tests
# 4 exploration tests + 7 preservation tests
```

PBT vitest frontend:
```bash
cd frontend
npm install
npm test
# 2 exploration + 6 preservation
```

Semua test fokus pada properti yang harus dijaga:
- Idempotency MQTT QoS=1 retransmit
- Geofence point-in-polygon stabil terhadap rotasi koordinat
- Escape HTML lengkap (XSS prevention)
- CSV field escaping (injeksi formula)
- Auth token tidak bocor ke localStorage di plaintext

## 📂 Struktur direktori

```
altivex_backend/
├── src/main.rs              # Backend Rust (auth, MQTT, WS, REST, serial bridge)
├── frontend/
│   ├── index.html           # Markup dashboard
│   ├── dashboard.css        # Modern Warm + dark mode
│   ├── dashboard.js         # Runtime (WS, Leaflet, Turf, alerts)
│   ├── GEO.json             # Jalur Cibodas / Gn Putri / Selabintana
│   ├── tests/               # PBT vitest
│   └── previews/            # 4 palette preview (didn't make it)
├── tests/                   # PBT Rust (proptest)
├── mosquitto/config/
│   └── mosquitto.conf       # auth required + persistence
├── deployment/
│   ├── README.md            # Runbook GCP
│   ├── bootstrap.sh         # Auto-generator .env + passwd
│   ├── Caddyfile            # Alternatif Caddy
│   ├── docker-compose.prod.yml
│   ├── esp32-templates/     # .ino plaintext + TLS
│   └── multi-project-host/
│       ├── README.md        # Caddy multi-project pattern (future)
│       └── AGENT_BRIEF.md   # Brief untuk AI agent project kedua
├── docker-compose.yml       # Compose dev + production base
├── Dockerfile               # Multi-stage Rust 1.90 → debian-slim
├── .env.example             # Template config (jangan commit .env asli)
└── DEPLOYMENT.md            # Catatan rotasi secret + history rewrite
```

## 🔐 Security checklist (sudah dipenuhi)

- ✅ `.env` tidak ter-commit (`.gitignore` cover `.env*`)
- ✅ `mosquitto/config/passwd` tidak ter-commit
- ✅ `allow_anonymous false` di mosquitto, password file wajib
- ✅ Auth Bearer token untuk semua endpoint mutating
- ✅ Payload validation (lat/lon range + finite + non-zero, id ≤50 char)
- ✅ JSON injection guard di outbound serial command (pakai `serde_json::to_vec`)
- ✅ XSS prevention via `escapeHtml` di seluruh `innerHTML`
- ✅ CSV injection prevention via `csvField`
- ✅ Idempotent INSERT (`ON CONFLICT DO NOTHING`) untuk MQTT QoS=1 retransmit
- ✅ MQTT reconnect exponential backoff dengan rebuild client (rumqttc B6)
- ✅ Container non-root user
- ✅ Reverse proxy WS upgrade headers terkonfigurasi
- ✅ HSTS + nosniff + X-Frame-Options DENY di response

⚠️ **Catatan rotasi**: `.env` lama pernah ter-commit di `a3f41c2` lalu dibersihkan di `78adfd9`. Detail rotasi di `DEPLOYMENT.md`.

## 🤝 Kontribusi

Project ini dibangun dengan metodologi spec-driven (lihat `.kiro/specs/altivex-critical-fixes/`):

1. **Bug condition C(X)** — formal spec dari setiap bug
2. **Exploration test** — PBT yang harus FAIL pada kode bermasalah
3. **Preservation test** — PBT yang harus tetap PASS setelah fix
4. **Fix implementation**
5. **Verification** — exploration sekarang PASS, preservation tetap PASS

Selama bug condition belum di-encode jadi PBT, jangan klaim fix selesai.

## 📝 Lisensi

MIT — lihat `LICENSE`.

## 🙏 Acknowledgment

- Taman Nasional Gunung Gede Pangrango — referensi jalur dan waypoint
- Komunitas pendaki Pangrango yang memberi feedback uji lapangan
- Heltec Basecamp — radio gateway LoRa untuk daerah tanpa sinyal seluler

---

<div align="center">
<sub>Dibangun dengan ❤️ di Sukabumi · v0.1.0 · 2026</sub>
</div>
