# ESP32 firmware templates — ALTIVEX

Tiga template firmware untuk node pendaki/pengguna demo (Heltec WiFi
LoRa V3 atau ESP32 generic). Pilih satu sesuai kebutuhan deployment.

| File | Port | Enkripsi | Untuk |
|---|---|---|---|
| `altivex_basic_mqtt.ino` | 1883 | ❌ Plaintext | Testing lab / Wi-Fi internal (PROD broker). |
| `altivex_tls_mqtt.ino`   | 8883 | ✅ TLS (Let's Encrypt) | Produksi di jaringan publik. |
| `altivex_demo_situgede.ino` | 1885 | ❌ Plaintext | Demo presentation di altivex-demo.duckdns.org dengan **GPS simulator** (loop CIFOR-Situgede 2.71 km). |

## Otentikasi

Kedua template **WAJIB** pakai `MQTT_USERNAME` + `MQTT_PASSWORD` yang
match dengan `mosquitto/config/passwd` di server. Mosquitto sekarang
`allow_anonymous false` sejak Bug B2 / Task 3.7 — broker akan tolak
koneksi yang tidak otentikasi.

`API_AUTH_TOKEN` (yang dipakai dashboard browser) **TIDAK** dipakai
device MQTT — itu hanya untuk REST endpoint mutating
(`POST /api/sensor`, `POST /api/alert`, dll.). Device cuma butuh
publish ke topic `altivex/sensor/data` lewat MQTT, bukan REST.

## Library Arduino IDE

Install via **Sketch → Include Library → Manage Libraries**:

1. **PubSubClient** by Nick O'Leary — MQTT client (≥ 2.8)
2. **ArduinoJson** by Benoit Blanchon — JSON encoder (≥ 6.21)
3. **TinyGPSPlus** by Mikal Hart — NMEA parser (≥ 1.0.3)

Untuk Heltec V3, install juga board package:

```
Tools → Board → Boards Manager → "Heltec ESP32 Series Dev-boards"
```

## Pin assignment default (ubah sesuai board Anda)

| Function | Pin |
|---|---|
| GPS NEO-6M TX → ESP32 RX | GPIO 16 (Serial2 RX) |
| GPS NEO-6M RX → ESP32 TX | GPIO 17 (Serial2 TX) |
| Vibration motor | GPIO 13 (active HIGH) |

## Quick start (basic, plaintext)

1. Edit `altivex_basic_mqtt.ino`, ganti 5 baris ini:
   ```cpp
   const char* WIFI_SSID     = "...";
   const char* WIFI_PASSWORD = "...";
   const char* MQTT_HOST     = "altivex-pangrango.duckdns.org";
   const char* MQTT_USERNAME = "altivex_prod";
   const char* MQTT_PASSWORD = "GANTI_DENGAN_MQTT_PASSWORD_DARI_DOTENV";
   const char* DEVICE_ID     = "ALAT-001";   // unik per alat
   ```

2. Pastikan server expose port 1883:
   ```yaml
   # docker-compose.yml
   mosquitto:
     ports:
       - "1883:1883"
   ```
   Plus firewall GCP buka port 1883.

3. Test dari laptop dulu sebelum flash:
   ```bash
   mosquitto_pub -h altivex-pangrango.duckdns.org -p 1883 \
       -u altivex_prod -P 'YOUR_MQTT_PASSWORD' \
       -t altivex/sensor/data \
       -m '{"id_perangkat":"TEST","latitude":-6.7711,"longitude":106.96}'
   ```
   Buka dashboard, marker "TEST" muncul di peta → siap flash.

4. Compile + upload via Arduino IDE.

## Quick start (TLS, produksi)

Lihat block "Catatan deployment TLS di server" di akhir
`altivex_tls_mqtt.ino` untuk dua cara setup TLS di sisi broker
(Mosquitto handle langsung VS reverse stream nginx).

## Quick start (DEMO Situgede dengan GPS simulator)

`altivex_demo_situgede.ino` punya 2 mode:

**Mode 1 — Simulator (default, `SIMULATE_GPS = true`)**
Tidak butuh GPS hardware. ESP32 generate posisi random di loop
Situgede berdasarkan waypoint di GEO.json. Ideal untuk demo presentasi.

**Mode 2 — Real GPS (`SIMULATE_GPS = false`)**
Pakai NEO-6M asli di Serial2, sama seperti basic template tapi konek
ke broker demo.

1. Ambil credential demo dari VM:
   ```bash
   ssh user@vm "grep -E '^(MQTT_USERNAME|MQTT_PASSWORD)=' \
       ~/ALTIVEX/deployment/demo-branch/.env.demo"
   ```

2. Edit `altivex_demo_situgede.ino`:
   ```cpp
   const char* WIFI_SSID     = "GANTI_SSID_ANDA";
   const char* WIFI_PASSWORD = "GANTI_PASSWORD_WIFI";
   const char* MQTT_PASSWORD = "...";  // dari .env.demo
   const char* DEVICE_ID     = "DEMO-CIFOR-01";
   ```
   Catat: `MQTT_HOST` dan `MQTT_PORT` sudah di-set ke
   `altivex-demo.duckdns.org:1885` — jangan diubah.

3. Compile + upload. Buka Serial Monitor 115200 untuk lihat publish log.

4. Login ke `https://altivex-demo.duckdns.org/`, daftarkan pendaki baru
   dengan ID Perangkat persis sama dengan `DEVICE_ID` di firmware
   (mis. `DEMO-CIFOR-01`).

5. Marker akan bergerak smooth di peta mengikuti loop, dengan
   battery indicator yang turun pelan-pelan dari 100% ke 20%.

## Quick test tanpa ESP32 (PowerShell + mosquitto_pub)

Kalau belum punya ESP32 atau mau test dulu di laptop, ada simulator
PowerShell di `scripts/demo-publisher.ps1` yang publish identik
dengan firmware ESP32:

```powershell
# 1. Set MQTT password sekali per session
$env:ALTIVEX_DEMO_MQTT_PASSWORD = '<paste dari .env.demo>'

# 2. Default: DEMO-CIFOR-01, loop 10 menit, publish tiap 3 detik
.\scripts\demo-publisher.ps1

# Multi-device: buka 3 PowerShell window paralel:
.\scripts\demo-publisher.ps1 -DeviceId DEMO-CIFOR-01
.\scripts\demo-publisher.ps1 -DeviceId DEMO-CIFOR-02 -LoopMin 7
.\scripts\demo-publisher.ps1 -DeviceId DEMO-CIFOR-03 -LoopMin 15

# Dry run (preview payload tanpa publish)
.\scripts\demo-publisher.ps1 -DryRun
```

Skrip pakai `mosquitto_pub.exe` dari install
`C:\Program Files\mosquitto\` (download di
[mosquitto.org/download](https://mosquitto.org/download/)). Stop
dengan `Ctrl+C` di window PowerShell.

## Format payload

```json
{
  "id_perangkat": "ALAT-001",
  "latitude": -6.7711,
  "longitude": 106.96,
  "battery": 87
}
```

`battery` adalah persen 0-100 (opsional). Backend akan render indicator
di dashboard. Out-of-range atau missing → di-treat sebagai null
("—" di UI).

Backend (Task 3.3 / Bug B8) akan reject:
- `latitude` di luar `[-90, 90]`
- `longitude` di luar `[-180, 180]`
- `id_perangkat` kosong atau panjang > 50 karakter
- `(latitude, longitude) ≈ (0, 0)` (NEO-6M lock loss)

Reject = log warning di server, tidak ada feedback ke device. Pastikan
firmware Anda tidak publish saat GPS belum lock.

## Troubleshooting

### `mqtt connect failed (state=4)` — bad credentials

`MQTT_PASSWORD` di firmware tidak match `mosquitto/config/passwd` di
server. Kemungkinan:
- Password di `.env` baru saja di-rotate tapi `passwd` belum
  regenerate — lihat `DEPLOYMENT.md` § "Rotasi password MQTT".
- Typo / extra space di string firmware.

### `mqtt connect failed (state=-2)` — di TLS

Sertifikat tidak valid. Tiga penyebab umum:
- ESP32 clock belum sync NTP (TLS strict tentang time-skew). Pastikan
  `configTime()` selesai sebelum `mqtt.connect()`.
- `ROOT_CA` di firmware tidak match issuer cert server (Let's Encrypt
  rotate root cert ~10 tahun, jarang).
- Hostname mismatch — `MQTT_HOST` harus match Common Name / SAN di cert.

### Posisi tidak muncul di dashboard tapi log MQTT bilang publish OK

Backend mungkin reject payload. Cek log server:

```bash
docker compose logs backend | grep "Payload sensor ditolak"
```

Kalau muncul, coordinate Anda di luar batas valid atau (0,0).
