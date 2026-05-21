// =====================================================================
// ALTIVEX DEMO — ESP32 Template untuk altivex-demo.duckdns.org
// Loop bersepeda Situgede (CIFOR -> Cilubang Malang -> Warung Tepi
// Hutan -> CIFOR), MQTT broker demo di port 1885.
// ---------------------------------------------------------------------
// Punya 2 mode operasi (toggle pakai SIMULATE_GPS):
//
// MODE 1 — SIMULATE_GPS = 1 (default untuk demo)
//   Tidak perlu GPS module. ESP32 simulasikan posisi pendaki yang
//   bersepeda di loop Situgede dengan interpolasi titik-titik dari
//   GEO.json. Ideal untuk demo presentasi tanpa hardware GPS.
//
// MODE 2 — SIMULATE_GPS = 0 (real hardware)
//   Pakai GPS NEO-6M asli di Serial2 (GPIO 16/17).
//
// Library yang perlu di-install via Arduino IDE Library Manager:
//   1. PubSubClient by Nick O'Leary       (>= 2.8)
//   2. ArduinoJson by Benoit Blanchon     (>= 6.21)
//   3. TinyGPSPlus by Mikal Hart          (>= 1.0.3) — hanya MODE 2
//
// Board: ESP32 Dev Module / Heltec WiFi LoRa V3 / Wemos D1 R32 / dll.
// Sudah tested di Arduino IDE 2.x dengan core esp32 v2.0.14+
// =====================================================================

#include <WiFi.h>
#include <PubSubClient.h>
#include <ArduinoJson.h>
#include <math.h>

// ====================================================================
// 1. KONFIGURASI — EDIT 4 BARIS DI BAWAH SEBELUM UPLOAD
// ====================================================================

// Toggle simulator (1) vs real GPS (0). PAKAI ANGKA, BUKAN true/false —
// preprocessor `#if` di sebagian toolchain ESP32 tidak parse keyword
// boolean dengan benar.
#define SIMULATE_GPS 1

// Wi-Fi yang punya akses internet
const char* WIFI_SSID     = "GANTI_SSID_ANDA";
const char* WIFI_PASSWORD = "GANTI_PASSWORD_WIFI";

// MQTT password — ambil dari deployment/demo-branch/.env.demo
//   ssh ke VM, jalankan:
//     grep MQTT_PASSWORD ~/ALTIVEX/deployment/demo-branch/.env.demo
const char* MQTT_PASSWORD = "GANTI_DENGAN_MQTT_PASSWORD_DARI_ENV_DEMO";

// Identitas perangkat — UNIK per ESP32 kalau pakai >1 device
const char* DEVICE_ID = "DEMO-CIFOR-01";

// ====================================================================
// 2. KONSTANTA — biasanya tidak perlu diubah
// ====================================================================

// Broker demo (port 1885 — beda dari prod yang 1883)
const char*    MQTT_HOST     = "altivex-demo.duckdns.org";
const uint16_t MQTT_PORT     = 1885;
const char*    MQTT_USERNAME = "altivex_demo";
const char*    TOPIC_PUB     = "altivex/sensor/data";

// Frekuensi publish posisi (ms). 3 detik = 1 update tiap ~12m di
// kecepatan sepeda 15 km/h, smooth di peta.
const uint32_t PUBLISH_INTERVAL_MS = 3000;

// Simulator: 1 loop selesai berapa lama. Loop 2.71 km, 10 menit =
// ~16 km/h (jogging/cycling santai).
const uint32_t SIMULATOR_LOOP_DURATION_MS = 600000;  // 10 menit

// Reconnect intervals (ms). Backoff untuk Wi-Fi + MQTT.
const uint32_t WIFI_CHECK_INTERVAL_MS    = 5000;     // 5s
const uint32_t MQTT_RETRY_DELAY_MS       = 2000;     // 2s
const uint8_t  MQTT_MAX_RETRIES_PER_LOOP = 3;        // 3 percobaan, lalu lanjut

// Pin LED status (built-in LED ESP32). Indikasi:
//   - Off:     belum konek Wi-Fi
//   - Slow blink (1Hz): konek Wi-Fi, belum MQTT
//   - On solid: Wi-Fi + MQTT OK, publishing
const uint8_t STATUS_LED_PIN = 2;  // ESP32 dev board built-in LED

// ====================================================================
// 3. HARDWARE PIN (hanya dipakai kalau SIMULATE_GPS = 0)
// ====================================================================
constexpr uint8_t  GPS_RX_PIN = 16;  // ESP32 RX <- GPS TX
constexpr uint8_t  GPS_TX_PIN = 17;  // ESP32 TX -> GPS RX
constexpr uint32_t GPS_BAUD   = 9600;

// ====================================================================
// 4. WAYPOINT LOOP CIFOR-SITUGEDE
//    Disederhanakan dari deployment/demo-branch/frontend-override/
//    GEO.json. ESP32 interpolasi linear antar waypoint untuk gerakan
//    smooth.
// ====================================================================
struct Waypoint { double lng; double lat; };
const Waypoint LOOP_WAYPOINTS[] = {
    // Start: Jl. CIFOR
    { 106.7518232, -6.5546282 },
    // Ke barat-utara via Jl. CIFOR -> Jl. Cilubang Malang
    { 106.7510000, -6.5540000 },
    { 106.7498000, -6.5532000 },
    { 106.7482000, -6.5524000 },
    { 106.7469000, -6.5519000 },
    // Waypoint 2: Jl. Cilubang Malang No.37
    { 106.7457227, -6.5517073 },
    // Ke selatan-timur menuju Jl. Rawajaha
    { 106.7462000, -6.5524000 },
    { 106.7470000, -6.5532000 },
    { 106.7480000, -6.5540000 },
    { 106.7490000, -6.5547000 },
    { 106.7500000, -6.5550000 },
    // Waypoint 3: Warung Tepi Hutan (Jl. Rawajaha)
    { 106.7507053, -6.5551558 },
    // Kembali ke Jl. CIFOR
    { 106.7510000, -6.5549000 },
    { 106.7515000, -6.5547000 },
    { 106.7518232, -6.5546282 }   // closed loop — match start
};
const size_t LOOP_WAYPOINT_COUNT = sizeof(LOOP_WAYPOINTS) / sizeof(LOOP_WAYPOINTS[0]);

// ====================================================================
// 5. STATE GLOBAL
// ====================================================================
WiFiClient    netClient;
PubSubClient  mqtt(netClient);

#if SIMULATE_GPS == 0
  #include <TinyGPSPlus.h>
  TinyGPSPlus    gps;
  HardwareSerial gpsSerial(2);
#endif

uint32_t lastPublishMs   = 0;
uint32_t simulatorStartMs = 0;
uint32_t lastWifiCheckMs = 0;
uint32_t lastLedToggleMs = 0;
bool     ledState        = false;
uint32_t publishCount    = 0;

// ====================================================================
// Helper: Wi-Fi connect (blocking pertama kali, non-blocking
// setelahnya). Setelah pertama kali konek, loop() akan re-check
// status setiap WIFI_CHECK_INTERVAL_MS dan trigger reconnect kalau
// drop, tanpa block main loop terlalu lama.
// ====================================================================
bool connectWifi(uint32_t timeoutMs = 30000) {
    if (WiFi.status() == WL_CONNECTED) return true;

    Serial.printf("[wifi] Connecting to '%s' ...\n", WIFI_SSID);
    WiFi.mode(WIFI_STA);
    WiFi.disconnect(true);  // clear stale session
    delay(100);
    WiFi.begin(WIFI_SSID, WIFI_PASSWORD);

    uint32_t start = millis();
    while (WiFi.status() != WL_CONNECTED) {
        if (millis() - start > timeoutMs) {
            Serial.println("\n[wifi] TIMEOUT. Akan retry di loop().");
            return false;
        }
        delay(500);
        Serial.print(".");
    }
    Serial.printf("\n[wifi] Connected. IP=%s, RSSI=%d dBm\n",
                  WiFi.localIP().toString().c_str(), WiFi.RSSI());
    return true;
}

// ====================================================================
// Helper: MQTT connect dengan limited retries supaya gak block loop()
// kalau broker offline. Return true kalau sukses, false kalau perlu
// retry di iterasi berikut.
// ====================================================================
bool connectMqtt() {
    if (mqtt.connected()) return true;

    for (uint8_t attempt = 0; attempt < MQTT_MAX_RETRIES_PER_LOOP; attempt++) {
        Serial.printf("[mqtt] Connecting %s:%u as '%s' (attempt %u/%u) ...\n",
                      MQTT_HOST, MQTT_PORT, MQTT_USERNAME,
                      attempt + 1, MQTT_MAX_RETRIES_PER_LOOP);

        // Client ID unique: DEVICE_ID + chip MAC (last 4 bytes)
        char clientId[40];
        uint64_t mac = ESP.getEfuseMac();
        snprintf(clientId, sizeof(clientId), "%s-%08X",
                 DEVICE_ID, (uint32_t)(mac & 0xFFFFFFFF));

        if (mqtt.connect(clientId, MQTT_USERNAME, MQTT_PASSWORD)) {
            Serial.printf("[mqtt] Connected as clientId='%s'\n", clientId);
            return true;
        }

        // PubSubClient state codes:
        //  -4: timeout, -3: lost, -2: connect failed, -1: disconnected
        //   1: bad protocol, 2: bad client ID, 3: server unavailable
        //   4: bad credentials, 5: not authorized
        Serial.printf("[mqtt] Failed (state=%d). ", mqtt.state());
        if (attempt + 1 < MQTT_MAX_RETRIES_PER_LOOP) {
            Serial.printf("Retry in %ums.\n", MQTT_RETRY_DELAY_MS);
            delay(MQTT_RETRY_DELAY_MS);
        } else {
            Serial.println("Akan retry di iterasi loop berikutnya.");
        }
    }
    return false;
}

// ====================================================================
// Simulator: hitung posisi saat ini (lat/lng) berdasarkan progress
// di loop. Linear interpolation antar dua waypoint sekitarnya.
// ====================================================================
void getSimulatedPosition(double& outLat, double& outLng) {
    uint32_t now     = millis();
    uint32_t elapsed = (now - simulatorStartMs) % SIMULATOR_LOOP_DURATION_MS;
    double   frac    = (double)elapsed / (double)SIMULATOR_LOOP_DURATION_MS;

    size_t segCount = LOOP_WAYPOINT_COUNT - 1;
    double segFrac  = frac * (double)segCount;
    size_t segIdx   = (size_t)floor(segFrac);
    if (segIdx >= segCount) segIdx = segCount - 1;
    double t = segFrac - (double)segIdx;

    const Waypoint& a = LOOP_WAYPOINTS[segIdx];
    const Waypoint& b = LOOP_WAYPOINTS[segIdx + 1];

    outLng = a.lng + (b.lng - a.lng) * t;
    outLat = a.lat + (b.lat - a.lat) * t;
}

// ====================================================================
// Battery decay simulator: 100 -> 20, drop 1% per 30 sec, stop di 20.
// Dipakai untuk demo low-battery notification di dashboard.
// ====================================================================
int getSimulatedBattery() {
    int drop  = millis() / 30000;
    int level = 100 - drop;
    if (level < 20) level = 20;
    return level;
}

// ====================================================================
// Update LED status berdasarkan koneksi state.
//   - Off:     Wi-Fi belum konek
//   - Slow blink (1Hz): Wi-Fi OK, MQTT belum konek
//   - On solid: Wi-Fi + MQTT OK
// ====================================================================
void updateStatusLed() {
    bool wifiOk = (WiFi.status() == WL_CONNECTED);
    bool mqttOk = wifiOk && mqtt.connected();

    if (mqttOk) {
        digitalWrite(STATUS_LED_PIN, HIGH);  // solid on
    } else if (wifiOk) {
        // Slow blink 1Hz
        if (millis() - lastLedToggleMs >= 500) {
            ledState = !ledState;
            digitalWrite(STATUS_LED_PIN, ledState ? HIGH : LOW);
            lastLedToggleMs = millis();
        }
    } else {
        digitalWrite(STATUS_LED_PIN, LOW);  // off
    }
}

// ====================================================================
// Publish JSON payload — schema MUST match struct IncomingData di
// backend Rust (main.rs):
//   {"id_perangkat": "...", "latitude": ..., "longitude": ..., "battery": ...}
// ====================================================================
bool publishPosition(double lat, double lng, int battery) {
    StaticJsonDocument<160> doc;
    doc["id_perangkat"] = DEVICE_ID;
    doc["latitude"]     = lat;
    doc["longitude"]    = lng;
    doc["battery"]      = battery;

    char buf[160];
    size_t n = serializeJson(doc, buf, sizeof(buf));

    if (mqtt.publish(TOPIC_PUB, (const uint8_t*)buf, n, /*retain=*/false)) {
        publishCount++;
        Serial.printf("[mqtt] #%u >> %s\n", (unsigned)publishCount, buf);
        return true;
    } else {
        Serial.printf("[mqtt] Publish FAILED (state=%d).\n", mqtt.state());
        return false;
    }
}

// ====================================================================
// Setup
// ====================================================================
void setup() {
    Serial.begin(115200);
    delay(500);

    pinMode(STATUS_LED_PIN, OUTPUT);
    digitalWrite(STATUS_LED_PIN, LOW);

    Serial.println();
    Serial.println("============================================");
    Serial.println("ALTIVEX DEMO — Situgede Cycling Loop");
    Serial.println("============================================");
    Serial.printf("Device ID:     %s\n", DEVICE_ID);
    Serial.printf("Mode:          %s\n",
        SIMULATE_GPS ? "GPS SIMULATOR" : "REAL GPS HARDWARE");
    Serial.printf("Broker:        %s:%u\n", MQTT_HOST, MQTT_PORT);
    Serial.printf("Topic:         %s\n", TOPIC_PUB);
    Serial.printf("Publish every: %ums\n", PUBLISH_INTERVAL_MS);
#if SIMULATE_GPS
    Serial.printf("Loop duration: %us (%.1f km/h)\n",
        SIMULATOR_LOOP_DURATION_MS / 1000,
        2.71 / (SIMULATOR_LOOP_DURATION_MS / 3600000.0));
#endif
    Serial.println("============================================");

#if SIMULATE_GPS == 0
    gpsSerial.begin(GPS_BAUD, SERIAL_8N1, GPS_RX_PIN, GPS_TX_PIN);
    Serial.printf("[gps] NEO-6M aktif di Serial2 (RX=GPIO%u, TX=GPIO%u, baud=%u)\n",
                  GPS_RX_PIN, GPS_TX_PIN, GPS_BAUD);
#endif

    connectWifi();
    mqtt.setServer(MQTT_HOST, MQTT_PORT);
    mqtt.setKeepAlive(30);
    mqtt.setBufferSize(256);  // default 128 cukup, tapi kasih breathing room
    connectMqtt();

    simulatorStartMs = millis();
    lastWifiCheckMs  = millis();
}

// ====================================================================
// Loop
// ====================================================================
void loop() {
    uint32_t now = millis();

    // 1. Wi-Fi watchdog — kalau drop, reconnect non-blocking.
    if (now - lastWifiCheckMs >= WIFI_CHECK_INTERVAL_MS) {
        lastWifiCheckMs = now;
        if (WiFi.status() != WL_CONNECTED) {
            Serial.println("[wifi] Disconnected. Reconnecting...");
            connectWifi(10000);  // 10s timeout, gak block forever
        }
    }

    // 2. MQTT watchdog — kalau Wi-Fi OK tapi MQTT disconnected,
    //    re-connect (limited retries supaya gak block loop terlalu lama).
    if (WiFi.status() == WL_CONNECTED && !mqtt.connected()) {
        connectMqtt();
    }

    // 3. Process MQTT keep-alive (PubSubClient internal heartbeat).
    if (mqtt.connected()) {
        mqtt.loop();
    }

    // 4. Update LED status.
    updateStatusLed();

#if SIMULATE_GPS == 0
    // 5. Feed GPS parser dari Serial2 (real hardware mode).
    while (gpsSerial.available() > 0) {
        gps.encode(gpsSerial.read());
    }
#endif

    // 6. Throttle publish — skip kalau belum waktunya atau MQTT down.
    if (now - lastPublishMs < PUBLISH_INTERVAL_MS) return;
    if (!mqtt.connected()) return;

    double lat, lng;

#if SIMULATE_GPS
    // Simulator mode: posisi di-interpolasi dari LOOP_WAYPOINTS
    getSimulatedPosition(lat, lng);
#else
    // Real GPS mode: skip kalau belum lock
    if (!gps.location.isValid()) {
        Serial.printf("[gps] Waiting for fix... (sat=%u, hdop=%u)\n",
                      (unsigned)gps.satellites.value(),
                      (unsigned)gps.hdop.value());
        lastPublishMs = now;  // tetap throttle, jangan flood log
        return;
    }
    lat = gps.location.lat();
    lng = gps.location.lng();
    if (fabs(lat) < 1e-6 && fabs(lng) < 1e-6) {
        Serial.println("[gps] Lock-loss anomaly (0,0), skip publish.");
        lastPublishMs = now;
        return;
    }
#endif

    int battery = getSimulatedBattery();
    if (publishPosition(lat, lng, battery)) {
        lastPublishMs = now;
    }
    // Kalau publish gagal, jangan update lastPublishMs supaya
    // iterasi loop berikutnya retry segera.
}

// =====================================================================
// CARA PAKAI — DEMO WORKFLOW
// ---------------------------------------------------------------------
// 1. Edit 4 baris di section KONFIGURASI:
//    - WIFI_SSID
//    - WIFI_PASSWORD
//    - MQTT_PASSWORD       (dari .env.demo, lihat di bawah)
//    - DEVICE_ID           (kalau >1 ESP32 demo)
//
// 2. Ambil MQTT_PASSWORD dari VM:
//
//      ssh user@<vm-ip>
//      grep MQTT_PASSWORD ~/ALTIVEX/deployment/demo-branch/.env.demo
//
// 3. Compile + upload via Arduino IDE:
//    - Tools > Board: ESP32 Dev Module (atau Heltec WiFi LoRa V3, dll.)
//    - Tools > Upload Speed: 921600
//    - Pilih COM port yang sesuai
//    - Klik Upload
//
// 4. Buka Serial Monitor 115200. Yang harus muncul:
//
//      ============================================
//      ALTIVEX DEMO — Situgede Cycling Loop
//      ============================================
//      Device ID:     DEMO-CIFOR-01
//      Mode:          GPS SIMULATOR
//      Broker:        altivex-demo.duckdns.org:1885
//      ...
//      [wifi] Connected. IP=192.168.x.x, RSSI=-XX dBm
//      [mqtt] Connecting altivex-demo.duckdns.org:1885 as 'altivex_demo' ...
//      [mqtt] Connected as clientId='DEMO-CIFOR-01-XXXXXXXX'
//      [mqtt] #1 >> {"id_perangkat":"DEMO-CIFOR-01","latitude":-6.5546,...}
//
//    Indikator LED on-board:
//      Off                = Wi-Fi belum konek
//      Slow blink (1Hz)   = Wi-Fi OK, MQTT belum konek
//      On solid           = Wi-Fi + MQTT OK, publishing
//
// 5. Buka dashboard demo: https://altivex-demo.duckdns.org/
//    Login dengan BASECAMP_USERNAME/PASSWORD dari .env.demo.
//    Daftarkan pendaki:
//      Nama: Demo Pendaki 1
//      ID Perangkat: DEMO-CIFOR-01    (HARUS persis match DEVICE_ID di .ino)
//      Telepon Darurat: +6281234567890
//
// 6. Marker bergerak di peta mengikuti loop Situgede dengan
//    kecepatan ~16 km/h (loop 2.71 km dalam 10 menit).
//
// ---------------------------------------------------------------------
// MULTI-DEVICE DEMO
// ---------------------------------------------------------------------
//   Flash 3 ESP32 dengan DEVICE_ID berbeda (DEMO-CIFOR-01,
//   DEMO-CIFOR-02, DEMO-CIFOR-03). Daftarkan ketiganya di dashboard.
//   Hasilnya: 3 pendaki bergerak paralel di Situgede dengan
//   kecepatan sama tapi posisi awal berbeda (efek tergantung waktu
//   power-on tiap ESP32).
//
//   Untuk variasi kecepatan, edit SIMULATOR_LOOP_DURATION_MS di
//   masing-masing flash (mis. 300000 = 5 menit untuk yang cepat,
//   900000 = 15 menit untuk yang lambat).
//
// ---------------------------------------------------------------------
// TEST GEOFENCING (OUT-OF-BOUNDS ALERT)
// ---------------------------------------------------------------------
//   Tambah satu waypoint di luar koridor di array LOOP_WAYPOINTS:
//
//     { 106.7300000, -6.5400000 },   // di luar geofence buffer
//
//   Re-flash. Saat marker melewati titik itu, dashboard trigger
//   alert "KELUAR KORIDOR" dan kartu pendaki pindah ke Alert Sidebar.
//
// ---------------------------------------------------------------------
// TROUBLESHOOTING
// ---------------------------------------------------------------------
//   - LED slow blink terus, log "[mqtt] Failed (state=4)"
//        => MQTT_PASSWORD salah. Re-grep dari .env.demo, paste ulang.
//
//   - "[mqtt] Failed (state=-2)" terus
//        => Broker tidak reachable. Cek port 1885 sudah dibuka di
//           firewall GCP. Test dari laptop:
//             mosquitto_pub -h altivex-demo.duckdns.org -p 1885 \
//                 -u altivex_demo -P 'YOUR_PASSWORD' \
//                 -t altivex/sensor/data -m '{"test":1}' -d
//
//   - Dashboard tidak menampilkan marker meski Serial print "[mqtt] >>"
//        => Pendaki belum terdaftar. Login dashboard, daftarkan
//           pendaki dengan ID Perangkat persis = DEVICE_ID.
//
//   - Wi-Fi sering disconnect
//        => Cek RSSI di Serial. Kalau lebih buruk dari -75 dBm,
//           pindah ESP32 lebih dekat ke router atau pakai antenna
//           eksternal.
// =====================================================================
