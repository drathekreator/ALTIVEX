// =====================================================================
// ALTIVEX DEMO — ESP32 Template untuk altivex-demo.duckdns.org
// Loop bersepeda Situgede (CIFOR -> Cilubang Malang -> Warung Tepi
// Hutan -> CIFOR), MQTT broker demo di port 1885.
// ---------------------------------------------------------------------
// Punya 2 mode operasi (toggle pakai SIMULATE_GPS):
//
// MODE 1 — SIMULATE_GPS = true (default untuk demo)
//   Tidak perlu GPS module. ESP32 simulasikan posisi pendaki yang
//   bersepeda di loop Situgede dengan interpolasi titik-titik dari
//   GEO.json. Ideal untuk demo presentasi tanpa hardware GPS.
//
// MODE 2 — SIMULATE_GPS = false (real hardware)
//   Pakai GPS NEO-6M asli di Serial2 (GPIO 16/17). Sama seperti
//   altivex_basic_mqtt.ino, tapi konek ke broker demo.
//
// Library yang sama dengan template prod:
//   1. PubSubClient by Nick O'Leary       (≥ 2.8)
//   2. ArduinoJson by Benoit Blanchon     (≥ 6.21)
//   3. TinyGPSPlus by Mikal Hart          (≥ 1.0.3) — hanya untuk MODE 2
// =====================================================================

#include <WiFi.h>
#include <PubSubClient.h>
#include <ArduinoJson.h>
#include <math.h>

// --- KONFIGURASI -----------------------------------------------------
// Toggle simulator vs real GPS
#define SIMULATE_GPS  true

// Wi-Fi (GANTI dengan Wi-Fi yang punya akses internet)
const char* WIFI_SSID     = "GANTI_SSID_ANDA";
const char* WIFI_PASSWORD = "GANTI_PASSWORD_WIFI";

// MQTT broker DEMO (port 1885 — beda dari prod yang 1883)
const char* MQTT_HOST     = "altivex-demo.duckdns.org";
const uint16_t MQTT_PORT  = 1885;

// MQTT credential demo — AMBIL DARI deployment/demo-branch/.env.demo
// Jangan pakai credential prod di sini.
const char* MQTT_USERNAME = "altivex_demo";
const char* MQTT_PASSWORD = "GANTI_DENGAN_MQTT_PASSWORD_DARI_ENV_DEMO";

// Identitas perangkat (boleh ganti — tiap ESP32 demo unik)
const char* DEVICE_ID = "DEMO-CIFOR-01";
const char* TOPIC_PUB = "altivex/sensor/data";

// Frekuensi publish posisi (ms). 3 detik = 1 update tiap ~12 meter
// di kecepatan sepeda 15 km/h, smooth di peta.
const uint32_t PUBLISH_INTERVAL_MS = 3000;

// Simulator: berapa lama satu loop selesai (ms). Loop 2.71 km, kalau
// kita kasih 5 menit (300_000 ms) = ~33 km/h, fast cycling.
// Pakai 600_000 ms (10 menit) untuk demo tenang.
const uint32_t SIMULATOR_LOOP_DURATION_MS = 600000;

// --- HARDWARE PIN (hanya dipakai kalau SIMULATE_GPS = false) --------
constexpr uint8_t GPS_RX_PIN = 16;
constexpr uint8_t GPS_TX_PIN = 17;
constexpr uint32_t GPS_BAUD  = 9600;

// --- WAYPOINT LOOP CIFOR-SITUGEDE -----------------------------------
// Disederhanakan dari deployment/demo-branch/frontend-override/GEO.json.
// 12 titik kunci di loop, ESP32 interpolasi linear antar mereka untuk
// simulasi gerakan smooth. Lat/lng dalam derajat WGS84.
struct Waypoint { double lng; double lat; };
const Waypoint LOOP_WAYPOINTS[] = {
    // Start: Jl. CIFOR
    { 106.7518232, -6.5546282 },
    // Ke barat-utara via Jl. Cilubang Malang
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
    // Waypoint 3: Warung Tepi Hutan
    { 106.7507053, -6.5551558 },
    // Kembali ke Jl. CIFOR
    { 106.7510000, -6.5549000 },
    { 106.7515000, -6.5547000 },
    { 106.7518232, -6.5546282 }   // closed loop — sama dengan start
};
const size_t LOOP_WAYPOINT_COUNT = sizeof(LOOP_WAYPOINTS) / sizeof(LOOP_WAYPOINTS[0]);

// --- STATE GLOBAL ----------------------------------------------------
WiFiClient    netClient;
PubSubClient  mqtt(netClient);

#if !SIMULATE_GPS
#include <TinyGPSPlus.h>
TinyGPSPlus    gps;
HardwareSerial gpsSerial(2);
#endif

uint32_t lastPublishMs = 0;
uint32_t simulatorStartMs = 0;
int simulatedBattery = 100;   // mulai full, drop seiring waktu untuk demo

// =====================================================================
// Helpers — Wi-Fi + MQTT (sama seperti template prod)
// =====================================================================
void connectWifi() {
    Serial.printf("[wifi] Connecting to %s ...\n", WIFI_SSID);
    WiFi.mode(WIFI_STA);
    WiFi.begin(WIFI_SSID, WIFI_PASSWORD);
    while (WiFi.status() != WL_CONNECTED) {
        delay(500);
        Serial.print(".");
    }
    Serial.printf("\n[wifi] Connected. IP=%s, RSSI=%d\n",
                  WiFi.localIP().toString().c_str(), WiFi.RSSI());
}

void connectMqtt() {
    while (!mqtt.connected()) {
        Serial.printf("[mqtt] Connecting %s:%u as '%s' ...\n",
                      MQTT_HOST, MQTT_PORT, MQTT_USERNAME);
        char clientId[40];
        snprintf(clientId, sizeof(clientId), "%s-%llx",
                 DEVICE_ID, ESP.getEfuseMac());
        if (mqtt.connect(clientId, MQTT_USERNAME, MQTT_PASSWORD)) {
            Serial.println("[mqtt] Connected.");
        } else {
            Serial.printf("[mqtt] Failed (state=%d). Retry 5s.\n",
                          mqtt.state());
            delay(5000);
        }
    }
}

// =====================================================================
// Simulator: hitung posisi saat ini berdasarkan progress di loop.
// Strategi:
//   1. progress = (now - simulatorStartMs) % LOOP_DURATION
//   2. progress fraction (0.0 -> 1.0) menentukan segmen mana di
//      LOOP_WAYPOINTS yang lagi dilewati
//   3. Linear interpolasi antara dua waypoint sekitarnya
// =====================================================================
void getSimulatedPosition(double& outLat, double& outLng) {
    uint32_t now = millis();
    uint32_t elapsed = (now - simulatorStartMs) % SIMULATOR_LOOP_DURATION_MS;
    double frac = (double)elapsed / (double)SIMULATOR_LOOP_DURATION_MS;

    // Segment count = waypoint count - 1 (between consecutive waypoints)
    size_t segCount = LOOP_WAYPOINT_COUNT - 1;
    double segFrac = frac * (double)segCount;
    size_t segIdx = (size_t)floor(segFrac);
    if (segIdx >= segCount) segIdx = segCount - 1;
    double t = segFrac - (double)segIdx;

    const Waypoint& a = LOOP_WAYPOINTS[segIdx];
    const Waypoint& b = LOOP_WAYPOINTS[segIdx + 1];

    outLng = a.lng + (b.lng - a.lng) * t;
    outLat = a.lat + (b.lat - a.lat) * t;
}

// Battery decay simulator: dari 100 turun pelan-pelan ke 20, lalu
// stabil di 20 sampai user reset. Dipakai untuk demo low-battery
// notification di dashboard.
int getSimulatedBattery() {
    uint32_t now = millis();
    // Setiap 30 detik, drop 1%. Stop di 20 supaya gak mati total.
    int drop = now / 30000;
    int level = 100 - drop;
    if (level < 20) level = 20;
    simulatedBattery = level;
    return level;
}

// =====================================================================
// Publish JSON payload — schema match struct IncomingData di main.rs:
//   {"id_perangkat":"...","latitude":..., "longitude":..., "battery":..}
// =====================================================================
void publishPosition(double lat, double lng, int battery) {
    StaticJsonDocument<160> doc;
    doc["id_perangkat"] = DEVICE_ID;
    doc["latitude"]  = lat;
    doc["longitude"] = lng;
    doc["battery"]   = battery;

    char buf[160];
    size_t n = serializeJson(doc, buf, sizeof(buf));

    if (!mqtt.publish(TOPIC_PUB, (const uint8_t*)buf, n, /*retain=*/false)) {
        Serial.printf("[mqtt] Publish FAILED (state=%d).\n", mqtt.state());
    } else {
        Serial.printf("[mqtt] >> %s\n", buf);
    }
}

// =====================================================================
// Setup
// =====================================================================
void setup() {
    Serial.begin(115200);
    delay(500);

    Serial.println();
    Serial.println("============================================");
    Serial.printf("ALTIVEX DEMO — Device: %s\n", DEVICE_ID);
    Serial.printf("Mode: %s\n", SIMULATE_GPS ? "GPS SIMULATOR" : "REAL GPS HARDWARE");
    Serial.printf("Broker: %s:%u\n", MQTT_HOST, MQTT_PORT);
    Serial.println("============================================");

#if !SIMULATE_GPS
    gpsSerial.begin(GPS_BAUD, SERIAL_8N1, GPS_RX_PIN, GPS_TX_PIN);
    Serial.println("[gps] Real GPS NEO-6M aktif di Serial2.");
#endif

    connectWifi();
    mqtt.setServer(MQTT_HOST, MQTT_PORT);
    mqtt.setKeepAlive(30);
    connectMqtt();

    simulatorStartMs = millis();
}

// =====================================================================
// Loop
// =====================================================================
void loop() {
    // 1. Maintain MQTT connection.
    if (!mqtt.connected()) connectMqtt();
    mqtt.loop();

    // 2. Feed GPS parser kalau real hardware.
#if !SIMULATE_GPS
    while (gpsSerial.available() > 0) {
        gps.encode(gpsSerial.read());
    }
#endif

    // 3. Throttle publish.
    uint32_t now = millis();
    if (now - lastPublishMs < PUBLISH_INTERVAL_MS) return;
    lastPublishMs = now;

    double lat, lng;

#if SIMULATE_GPS
    // Simulator mode: posisi di-interpolasi dari LOOP_WAYPOINTS
    getSimulatedPosition(lat, lng);
#else
    // Real GPS mode: skip kalau belum lock
    if (!gps.location.isValid()) {
        Serial.printf("[gps] Waiting for fix... (sat=%lu, hdop=%lu)\n",
                      gps.satellites.value(), gps.hdop.value());
        return;
    }
    lat = gps.location.lat();
    lng = gps.location.lng();
    if (fabs(lat) < 1e-6 && fabs(lng) < 1e-6) {
        Serial.println("[gps] Lock-loss (0,0), skip.");
        return;
    }
#endif

    int battery = getSimulatedBattery();
    publishPosition(lat, lng, battery);
}

// =====================================================================
// Cara pakai (demo workflow)
// ---------------------------------------------------------------------
// 1. Edit credential di section KONFIGURASI:
//    - WIFI_SSID + WIFI_PASSWORD
//    - MQTT_PASSWORD (ambil dari output bootstrap-demo.sh atau:
//        ssh user@vm "grep MQTT_PASSWORD ~/ALTIVEX/deployment/demo-branch/.env.demo")
//
// 2. (Opsional) Pakai DEVICE_ID berbeda untuk tiap ESP32 supaya
//    multi-device test:
//        DEMO-CIFOR-01, DEMO-CIFOR-02, DEMO-CIFOR-03, ...
//
// 3. Compile + upload via Arduino IDE (Board: ESP32 Dev Module / Heltec
//    WiFi LoRa V3, baudrate 115200).
//
// 4. Buka Serial Monitor 115200. Yang harus muncul:
//        [wifi] Connecting to <SSID> ...
//        [wifi] Connected. IP=...
//        [mqtt] Connecting altivex-demo.duckdns.org:1885 as 'altivex_demo' ...
//        [mqtt] Connected.
//        [mqtt] >> {"id_perangkat":"DEMO-CIFOR-01","latitude":-6.5546,...}
//
// 5. Buka dashboard demo: https://altivex-demo.duckdns.org/
//    Login dengan BASECAMP_USERNAME/PASSWORD dari .env.demo.
//    Daftarkan pendaki baru:
//        Nama: Demo Pendaki 1
//        ID Perangkat: DEMO-CIFOR-01  (HARUS MATCH DEVICE_ID di .ino)
//        Telepon Darurat: +6281234567890
//
// 6. Marker bergerak di peta mengikuti loop Situgede dengan kecepatan
//    ~16 km/h (loop 2.71 km dalam 10 menit).
//
// 7. Untuk reset simulator: tekan tombol reset di ESP32, simulator
//    mulai lagi dari titik 1 (Jl. CIFOR).
//
// 8. Test alert geofencing: edit LOOP_WAYPOINTS di kode, tambah satu
//    titik di luar geofence corridor (misal lat -6.5400 atau lng
//    106.7400). Re-flash. Saat marker sampai di titik itu, dashboard
//    akan trigger alert "Out of Geofence".
// =====================================================================
