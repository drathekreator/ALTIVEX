// =====================================================================
// ALTIVEX — Template ESP32 (Heltec WiFi LoRa V3 atau ESP32 generic)
// Plaintext MQTT (port 1883) + GPS NEO-6M.
// ---------------------------------------------------------------------
// Gunakan template ini HANYA untuk:
//   - Testing di lab / Wi-Fi internal yang Anda kontrol.
//   - Dev iteration cepat.
//
// JANGAN deploy plaintext MQTT ke jaringan publik / 4G / Wi-Fi terbuka.
// `MQTT_PASSWORD` akan dikirim apa adanya dan bisa di-sniff oleh
// siapa pun di jalur jaringan. Untuk produksi, pakai
// `altivex_tls_mqtt.ino`.
// ---------------------------------------------------------------------
//
// LIBRARY (install via Arduino IDE → Library Manager):
//   1. PubSubClient by Nick O'Leary       (≥ 2.8)
//   2. ArduinoJson by Benoit Blanchon     (≥ 6.21)
//   3. TinyGPSPlus by Mikal Hart          (≥ 1.0.3)
//
// PIN ASSIGNMENT (Heltec V3 default — sesuaikan untuk board lain):
//   GPS NEO-6M TX → ESP32 GPIO 16 (Serial2 RX)
//   GPS NEO-6M RX → ESP32 GPIO 17 (Serial2 TX)
//   Vibration motor → GPIO 13 (active HIGH)
//
// PAYLOAD FORMAT (publish ke altivex/sensor/data):
//   {"id_perangkat":"ALAT-001","latitude":-6.7711,"longitude":106.96}
//
// Backend ALTIVEX (Task 3.3) menolak payload di luar range:
//   - latitude di luar [-90, 90] → di-skip + log warning
//   - longitude di luar [-180, 180] → di-skip + log warning
//   - id_perangkat empty atau > 50 char → di-skip
//   - (lat, lon) ≈ (0, 0) (NEO-6M lock loss) → di-skip
//
// SUBSCRIBE (untuk terima downlink alert):
//   Topic: altivex/alert/<id_perangkat> (opsional, lihat catatan akhir)
//   Saat ini downlink alert ALTIVEX dikirim via Serial dari basecamp,
//   bukan via MQTT. Subscribe di sini reserved untuk future.
// =====================================================================

#include <WiFi.h>
#include <PubSubClient.h>
#include <ArduinoJson.h>
#include <TinyGPSPlus.h>

// --- KONFIGURASI -----------------------------------------------------
// GANTI nilai di bawah dengan kredensial produksi Anda.

// Wi-Fi
const char* WIFI_SSID     = "GANTI_SSID_ANDA";
const char* WIFI_PASSWORD = "GANTI_PASSWORD_WIFI";

// MQTT broker
const char* MQTT_HOST     = "altivex-pangrango.duckdns.org"; // domain publik
const uint16_t MQTT_PORT  = 1883;
const char* MQTT_USERNAME = "altivex_prod";
const char* MQTT_PASSWORD = "GANTI_DENGAN_MQTT_PASSWORD_DARI_DOTENV";

// Identitas perangkat
const char* DEVICE_ID = "ALAT-001";   // unique per pendaki/alat
const char* TOPIC_PUB = "altivex/sensor/data";

// Frekuensi publish (ms) — backend cap WS broadcast 1 per 500ms,
// jadi 5-10 detik per device sudah cukup.
const uint32_t PUBLISH_INTERVAL_MS = 5000;

// --- HARDWARE PIN ---------------------------------------------------
constexpr uint8_t GPS_RX_PIN = 16;   // ESP32 RX ← GPS TX
constexpr uint8_t GPS_TX_PIN = 17;   // ESP32 TX → GPS RX
constexpr uint32_t GPS_BAUD  = 9600;
constexpr uint8_t VIBRATOR_PIN = 13;

// --- STATE GLOBAL ----------------------------------------------------
WiFiClient    netClient;
PubSubClient  mqtt(netClient);
TinyGPSPlus   gps;
HardwareSerial gpsSerial(2);

uint32_t lastPublishMs = 0;

// =====================================================================
// Helper: connect Wi-Fi (blocking, retry sampai sukses).
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

// =====================================================================
// Helper: connect MQTT (blocking, retry sampai sukses).
// Dipanggil dari setup() + loop() saat koneksi drop.
// =====================================================================
void connectMqtt() {
    while (!mqtt.connected()) {
        Serial.printf("[mqtt] Connecting to %s:%u as '%s' ...\n",
                      MQTT_HOST, MQTT_PORT, MQTT_USERNAME);
        // Client ID harus unik di broker — kalau dua device pakai ID
        // yang sama, mosquitto akan kick yang lama saat yang baru
        // connect. Kita pakai DEVICE_ID + chip ID untuk safety.
        char clientId[40];
        snprintf(clientId, sizeof(clientId), "%s-%llx",
                 DEVICE_ID, ESP.getEfuseMac());
        if (mqtt.connect(clientId, MQTT_USERNAME, MQTT_PASSWORD)) {
            Serial.println("[mqtt] Connected.");
        } else {
            // PubSubClient state codes:
            //   -4: connection timeout
            //   -3: connection lost
            //   -2: connect failed (wrong host/port)
            //   -1: disconnected
            //    0: connected
            //    1: bad protocol
            //    2: bad client ID
            //    3: server unavailable
            //    4: bad credentials
            //    5: not authorized
            Serial.printf("[mqtt] Failed (state=%d). Retry in 5s.\n",
                          mqtt.state());
            delay(5000);
        }
    }
}

// =====================================================================
// Bangun JSON payload + publish via MQTT.
// Wajib selaras dengan struct IncomingData di main.rs Rust:
//   {"id_perangkat":"...","latitude":..., "longitude":...}
// =====================================================================
void publishPosition(double lat, double lon) {
    StaticJsonDocument<128> doc;
    doc["id_perangkat"] = DEVICE_ID;
    doc["latitude"]  = lat;
    doc["longitude"] = lon;

    char buf[128];
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
    pinMode(VIBRATOR_PIN, OUTPUT);
    digitalWrite(VIBRATOR_PIN, LOW);

    gpsSerial.begin(GPS_BAUD, SERIAL_8N1, GPS_RX_PIN, GPS_TX_PIN);

    connectWifi();
    mqtt.setServer(MQTT_HOST, MQTT_PORT);
    // Buffer default PubSubClient = 256 byte. JSON payload ALTIVEX
    // mungil (~80 byte), aman tanpa naik buffer.
    mqtt.setKeepAlive(30);
    connectMqtt();
}

// =====================================================================
// Loop
// =====================================================================
void loop() {
    // 1. Maintain MQTT connection.
    if (!mqtt.connected()) connectMqtt();
    mqtt.loop();

    // 2. Feed GPS parser dari Serial2.
    while (gpsSerial.available() > 0) {
        gps.encode(gpsSerial.read());
    }

    // 3. Setiap PUBLISH_INTERVAL_MS, kirim posisi kalau GPS lock.
    uint32_t now = millis();
    if (now - lastPublishMs < PUBLISH_INTERVAL_MS) return;
    lastPublishMs = now;

    if (!gps.location.isValid()) {
        Serial.printf("[gps] Waiting for fix... (sat=%lu, hdop=%lu)\n",
                      gps.satellites.value(), gps.hdop.value());
        return;
    }

    double lat = gps.location.lat();
    double lon = gps.location.lng();

    // Defensive: skip (0, 0) di sisi device juga, walaupun backend
    // sudah punya guard. Mengurangi traffic + log noise.
    if (fabs(lat) < 1e-6 && fabs(lon) < 1e-6) {
        Serial.println("[gps] Lock-loss anomaly (0,0), skip.");
        return;
    }

    publishPosition(lat, lon);
}

// =====================================================================
// Catatan deployment
// ---------------------------------------------------------------------
// 1. Untuk pakai port 1883 plaintext di server Anda, edit
//    `altivex_backend/docker-compose.yml`:
//
//      mosquitto:
//        ports:
//          - "1883:1883"   # publish ke host
//
//    Lalu di GCP firewall rules, buka port 1883:
//
//      gcloud compute firewall-rules create altivex-mqtt \
//          --allow tcp:1883 \
//          --target-tags=mqtt-broker
//      gcloud compute instances add-tags <VM_NAME> \
//          --tags=mqtt-broker
//
//    Restart compose: `docker compose up -d`.
//
// 2. Test connect dari laptop dulu sebelum flash ke ESP32:
//
//      mosquitto_pub -h altivex-pangrango.duckdns.org -p 1883 \
//          -u altivex_prod -P 'YOUR_PASSWORD' \
//          -t altivex/sensor/data \
//          -m '{"id_perangkat":"TEST","latitude":-6.7711,"longitude":106.96}'
//
//    Buka dashboard, lihat marker TEST muncul di peta.
//
// 3. Untuk produksi sungguhan, pindah ke `altivex_tls_mqtt.ino` dan
//    aktifkan listener TLS di Mosquitto (8883) — plaintext password
//    di public Wi-Fi terlalu berisiko.
// =====================================================================
