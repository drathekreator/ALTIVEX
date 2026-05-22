// =====================================================================
// ALTIVEX DEMO — ESP32 firmware identik dengan produksi
// Untuk dashboard demo di altivex-demo.duckdns.org (peta loop CIFOR-
// Situgede, Bogor).
// ---------------------------------------------------------------------
// Hardware identik dengan produksi:
//   - ESP32 (Heltec WiFi LoRa V3 / generic ESP32 dev board)
//   - GPS NEO-6M di Serial2 (GPIO 16 = RX, GPIO 17 = TX)
//
// Yang berbeda dari produksi:
//   - MQTT broker: altivex-demo.duckdns.org:1885 (bukan port 1883)
//   - Credential: ambil dari deployment/demo-branch/.env.demo
//
// Yang sama persis dengan produksi:
//   - GPS NEO-6M asli (bukan simulator) — pendaki bawa device,
//     jalan/sepeda di Situgede, koordinat dari satelit
//   - WiFi/4G untuk publish MQTT (Bogor punya cell signal, beda
//     dengan Pangrango yang remote dan butuh LoRa basecamp)
//   - Payload schema sama: {id_perangkat, latitude, longitude}
//
// Library yang perlu di-install via Arduino IDE Library Manager:
//   1. PubSubClient by Nick O'Leary       (>= 2.8)
//   2. ArduinoJson by Benoit Blanchon     (>= 6.21)
//   3. TinyGPSPlus by Mikal Hart          (>= 1.0.3)
//
// Board: ESP32 Dev Module / Heltec WiFi LoRa V3 / dll.
// Tested: Arduino IDE 2.x, core esp32 v2.0.14+
// =====================================================================

#include <WiFi.h>
#include <PubSubClient.h>
#include <ArduinoJson.h>
#include <TinyGPSPlus.h>
#include <math.h>

// ====================================================================
// 1. KONFIGURASI — EDIT 4 BARIS DI BAWAH SEBELUM UPLOAD
// ====================================================================

// Wi-Fi yang punya akses internet (Wi-Fi rumah / tethering 4G HP)
const char* WIFI_SSID     = "GANTI_SSID_ANDA";
const char* WIFI_PASSWORD = "GANTI_PASSWORD_WIFI";

// MQTT password — ambil dari .env.demo di VM:
//   ssh user@<vm-ip>
//   grep MQTT_PASSWORD ~/ALTIVEX/deployment/demo-branch/.env.demo
const char* MQTT_PASSWORD = "GANTI_DENGAN_MQTT_PASSWORD_DARI_ENV_DEMO";

// Identitas perangkat — UNIK per ESP32 kalau pakai >1 device demo
const char* DEVICE_ID = "DEMO-CIFOR-01";

// ====================================================================
// 2. KONSTANTA BROKER DEMO — biasanya tidak perlu diubah
// ====================================================================
const char*    MQTT_HOST     = "altivex-demo.duckdns.org";
const uint16_t MQTT_PORT     = 1885;
const char*    MQTT_USERNAME = "altivex_demo";
const char*    TOPIC_PUB     = "altivex/sensor/data";

// Frekuensi publish posisi (ms). Smooth movement di peta tapi tidak
// flood broker. 5 detik = 1 update tiap ~20m di kecepatan sepeda
// 14 km/h.
const uint32_t PUBLISH_INTERVAL_MS = 5000;

// Reconnect intervals — non-blocking watchdog di main loop.
const uint32_t WIFI_CHECK_INTERVAL_MS    = 5000;
const uint32_t MQTT_RETRY_DELAY_MS       = 2000;
const uint8_t  MQTT_MAX_RETRIES_PER_LOOP = 3;

// LED status indicator (built-in di mayoritas dev board ESP32):
//   - Off:     belum konek Wi-Fi
//   - Slow blink (1Hz): Wi-Fi OK, MQTT belum konek
//   - On solid: Wi-Fi + MQTT OK, publishing aktif
const uint8_t STATUS_LED_PIN = 2;

// ====================================================================
// 3. HARDWARE PIN GPS NEO-6M
// ====================================================================
constexpr uint8_t  GPS_RX_PIN = 16;  // ESP32 RX <- GPS TX
constexpr uint8_t  GPS_TX_PIN = 17;  // ESP32 TX -> GPS RX
constexpr uint32_t GPS_BAUD   = 9600;

// ====================================================================
// 4. STATE GLOBAL
// ====================================================================
WiFiClient    netClient;
PubSubClient  mqtt(netClient);
TinyGPSPlus   gps;
HardwareSerial gpsSerial(2);

uint32_t lastPublishMs   = 0;
uint32_t lastWifiCheckMs = 0;
uint32_t lastLedToggleMs = 0;
bool     ledState        = false;
uint32_t publishCount    = 0;

// ====================================================================
// Helper: Wi-Fi connect dengan timeout supaya gak block forever.
// Setelah inisial connect, loop() yang panggil ini lagi tiap 5 detik
// kalau ter-deteksi disconnect (auto-reconnect).
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
// Helper: MQTT connect dengan limited retries (gak block loop kalau
// broker sedang offline). Return true kalau sukses.
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
// Update LED status berdasarkan state koneksi.
// ====================================================================
void updateStatusLed() {
    bool wifiOk = (WiFi.status() == WL_CONNECTED);
    bool mqttOk = wifiOk && mqtt.connected();

    if (mqttOk) {
        digitalWrite(STATUS_LED_PIN, HIGH);  // solid on
    } else if (wifiOk) {
        if (millis() - lastLedToggleMs >= 500) {  // 1Hz blink
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
//   {"id_perangkat": "...", "latitude": ..., "longitude": ...}
// ====================================================================
bool publishPosition(double lat, double lng) {
    StaticJsonDocument<128> doc;
    doc["id_perangkat"] = DEVICE_ID;
    doc["latitude"]     = lat;
    doc["longitude"]    = lng;

    char buf[128];
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
    Serial.println("ALTIVEX DEMO — Real GPS Hardware");
    Serial.println("============================================");
    Serial.printf("Device ID:     %s\n", DEVICE_ID);
    Serial.printf("Broker:        %s:%u\n", MQTT_HOST, MQTT_PORT);
    Serial.printf("Topic:         %s\n", TOPIC_PUB);
    Serial.printf("Publish every: %ums\n", PUBLISH_INTERVAL_MS);
    Serial.println("============================================");

    gpsSerial.begin(GPS_BAUD, SERIAL_8N1, GPS_RX_PIN, GPS_TX_PIN);
    Serial.printf("[gps] NEO-6M aktif di Serial2 (RX=GPIO%u, TX=GPIO%u, baud=%u)\n",
                  GPS_RX_PIN, GPS_TX_PIN, GPS_BAUD);

    connectWifi();
    mqtt.setServer(MQTT_HOST, MQTT_PORT);
    mqtt.setKeepAlive(30);
    mqtt.setBufferSize(256);
    connectMqtt();

    lastWifiCheckMs = millis();
}

// ====================================================================
// Loop
// ====================================================================
void loop() {
    uint32_t now = millis();

    // 1. Wi-Fi watchdog — non-blocking reconnect kalau drop.
    if (now - lastWifiCheckMs >= WIFI_CHECK_INTERVAL_MS) {
        lastWifiCheckMs = now;
        if (WiFi.status() != WL_CONNECTED) {
            Serial.println("[wifi] Disconnected. Reconnecting...");
            connectWifi(10000);
        }
    }

    // 2. MQTT watchdog — re-connect kalau Wi-Fi OK tapi MQTT down.
    if (WiFi.status() == WL_CONNECTED && !mqtt.connected()) {
        connectMqtt();
    }

    // 3. MQTT keep-alive heartbeat.
    if (mqtt.connected()) {
        mqtt.loop();
    }

    // 4. Status LED.
    updateStatusLed();

    // 5. Feed GPS parser dari Serial2.
    while (gpsSerial.available() > 0) {
        gps.encode(gpsSerial.read());
    }

    // 6. Throttle publish — skip kalau belum waktunya atau MQTT down.
    if (now - lastPublishMs < PUBLISH_INTERVAL_MS) return;
    if (!mqtt.connected()) return;

    // 7. GPS lock check.
    if (!gps.location.isValid()) {
        Serial.printf("[gps] Waiting for fix... (sat=%u, hdop=%u)\n",
                      (unsigned)gps.satellites.value(),
                      (unsigned)gps.hdop.value());
        lastPublishMs = now;  // throttle log spam
        return;
    }

    double lat = gps.location.lat();
    double lng = gps.location.lng();

    // 8. Defensive: skip (0,0) di sisi device juga (NEO-6M lock-loss
    //    anomaly). Backend juga punya guard, tapi mengurangi traffic.
    if (fabs(lat) < 1e-6 && fabs(lng) < 1e-6) {
        Serial.println("[gps] Lock-loss anomaly (0,0), skip publish.");
        lastPublishMs = now;
        return;
    }

    // 9. Publish.
    if (publishPosition(lat, lng)) {
        lastPublishMs = now;
    }
    // Kalau publish gagal, jangan update lastPublishMs supaya retry
    // langsung di iterasi loop berikutnya.
}

// =====================================================================
// CARA PAKAI — DEMO WORKFLOW
// ---------------------------------------------------------------------
// 1. Edit 4 baris di Section 1:
//    - WIFI_SSID
//    - WIFI_PASSWORD
//    - MQTT_PASSWORD       (dari .env.demo)
//    - DEVICE_ID           (kalau >1 ESP32 demo)
//
// 2. Ambil MQTT_PASSWORD dari VM:
//
//      ssh user@<vm-ip>
//      grep MQTT_PASSWORD ~/ALTIVEX/deployment/demo-branch/.env.demo
//
// 3. Sambung GPS NEO-6M ke ESP32:
//      GPS VCC  → ESP32 3.3V (atau 5V kalau modul-nya 5V tolerant)
//      GPS GND  → ESP32 GND
//      GPS TX   → ESP32 GPIO 16
//      GPS RX   → ESP32 GPIO 17
//
// 4. Compile + upload via Arduino IDE:
//    - Tools > Board: ESP32 Dev Module (atau Heltec WiFi LoRa V3)
//    - Tools > Upload Speed: 921600
//    - Pilih COM port
//    - Klik Upload
//
// 5. Buka Serial Monitor 115200. Yang harus muncul:
//
//      ============================================
//      ALTIVEX DEMO — Real GPS Hardware
//      ============================================
//      Device ID:     DEMO-CIFOR-01
//      Broker:        altivex-demo.duckdns.org:1885
//      ...
//      [gps] NEO-6M aktif di Serial2 (RX=GPIO16, TX=GPIO17, baud=9600)
//      [wifi] Connected. IP=192.168.x.x, RSSI=-XX dBm
//      [mqtt] Connecting altivex-demo.duckdns.org:1885 as 'altivex_demo' (attempt 1/3) ...
//      [mqtt] Connected as clientId='DEMO-CIFOR-01-XXXXXXXX'
//      [gps] Waiting for fix... (sat=0, hdop=99)
//      [gps] Waiting for fix... (sat=3, hdop=15)         <-- masih cari sinyal
//      [mqtt] #1 >> {"id_perangkat":"DEMO-CIFOR-01","latitude":-6.554628,"longitude":106.751823}
//
//    GPS first-fix outdoor biasanya 30-90 detik. Indoor: bisa lebih
//    lama atau gak dapat fix sama sekali. Pastikan GPS antenna outdoor.
//
//    LED on-board:
//      Off                = Wi-Fi belum konek
//      Slow blink (1Hz)   = Wi-Fi OK, MQTT belum konek
//      On solid           = Wi-Fi + MQTT OK, ready publish setelah GPS fix
//
// 6. Buka dashboard demo: https://altivex-demo.duckdns.org/
//    Login dengan BASECAMP_USERNAME/PASSWORD dari .env.demo.
//    Daftarkan pendaki:
//      Nama: Demo Pendaki 1
//      ID Perangkat: DEMO-CIFOR-01    (HARUS persis match DEVICE_ID di .ino)
//      Telepon Darurat: +6281234567890
//
// 7. Bawa ESP32 + GPS muter Situgede. Marker bergerak di peta sesuai
//    posisi GPS asli kamu.
//
// ---------------------------------------------------------------------
// MULTI-DEVICE DEMO
// ---------------------------------------------------------------------
//   Flash beberapa ESP32 dengan DEVICE_ID berbeda. Daftarkan tiap ID
//   di dashboard. Beberapa pendaki bergerak paralel di peta.
//
// ---------------------------------------------------------------------
// DEMO TANPA HARDWARE GPS
// ---------------------------------------------------------------------
//   Kalau belum punya GPS NEO-6M atau lagi rapat presentasi tanpa
//   bisa keluar gedung, pakai PowerShell/bash simulator yang sudah
//   disediakan:
//
//      # Dari laptop:
//      .\scripts\demo-publisher.ps1
//
//      # Dari VM:
//      ./scripts/demo-publisher.sh
//
//   Skrip itu generate posisi yang muter di loop Situgede tanpa GPS,
//   publish ke broker yang sama persis. Marker bergerak smooth di
//   dashboard.
//
// ---------------------------------------------------------------------
// TROUBLESHOOTING
// ---------------------------------------------------------------------
//   - "[mqtt] Failed (state=4)" terus
//        => MQTT_PASSWORD salah. Re-grep dari .env.demo, paste ulang.
//
//   - "[mqtt] Failed (state=-2)" terus
//        => Broker tidak reachable. Cek port 1885 sudah dibuka di
//           firewall GCP. Smoke test dari laptop:
//             mosquitto_pub -h altivex-demo.duckdns.org -p 1885 \
//                 -u altivex_demo -P 'YOUR_PASSWORD' \
//                 -t altivex/sensor/data -m '{"test":1}' -d
//
//   - "[gps] Waiting for fix..." berjam-jam, gak pernah valid
//        => GPS gak bisa lock satelit. Penyebab umum:
//           (a) GPS antenna terhalang (di dalam gedung / dalam tas)
//           (b) Wiring salah (RX/TX terbalik atau short)
//           (c) GPS power instabil (5V regulator overload, ganti supply)
//           (d) NEO-6M cold start butuh almanac download — diam outdoor
//               5-10 menit untuk first fix.
//
//   - Dashboard tidak menampilkan marker meski Serial print "[mqtt] >>"
//        => Pendaki belum terdaftar dengan ID yang sama. Login
//           dashboard, daftarkan pendaki dengan ID Perangkat persis =
//           DEVICE_ID di firmware.
//
//   - Wi-Fi sering disconnect saat outdoor
//        => Cek RSSI di Serial. Kalau lebih buruk dari -75 dBm,
//           pakai tethering 4G HP sebagai hotspot, atau pasang antenna
//           eksternal di ESP32.
// =====================================================================
