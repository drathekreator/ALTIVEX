// =====================================================================
// ALTIVEX — Template ESP32 dengan MQTT-over-TLS (port 8883)
// Untuk produksi: enkripsi penuh + sertifikat Let's Encrypt.
// ---------------------------------------------------------------------
//
// Kenapa TLS?
//   - Plaintext MQTT (port 1883) mengirim `MQTT_PASSWORD` apa adanya.
//     Siapa pun di jalur jaringan (Wi-Fi terbuka, ISP, warnet) bisa
//     sniff dan reuse credential.
//   - Posisi GPS pendaki juga sensitif — TLS encrypt seluruh payload.
//
// LIBRARY (install via Arduino IDE → Library Manager):
//   1. PubSubClient by Nick O'Leary       (≥ 2.8)
//   2. ArduinoJson by Benoit Blanchon     (≥ 6.21)
//   3. TinyGPSPlus by Mikal Hart          (≥ 1.0.3)
//
// PREREQ DI SERVER (sekali):
//   1. Mosquitto listener TLS aktif di port 8883 — lihat
//      `deployment/mosquitto.tls.conf` (template di repo).
//   2. Caddy / nginx forward port 8883/tcp ke container mosquitto,
//      ATAU broker langsung publish 8883:8883 dengan sertifikat
//      Let's Encrypt yang di-mount.
//   3. GCP firewall buka port 8883.
//
// =====================================================================

#include <WiFi.h>
#include <WiFiClientSecure.h>
#include <PubSubClient.h>
#include <ArduinoJson.h>
#include <TinyGPSPlus.h>

// --- KONFIGURASI -----------------------------------------------------
const char* WIFI_SSID     = "GANTI_SSID_ANDA";
const char* WIFI_PASSWORD = "GANTI_PASSWORD_WIFI";

const char* MQTT_HOST     = "altivex-pangrango.duckdns.org";
const uint16_t MQTT_PORT  = 8883;                    // TLS
const char* MQTT_USERNAME = "altivex_prod";
const char* MQTT_PASSWORD = "GANTI_DENGAN_MQTT_PASSWORD_DARI_DOTENV";

const char* DEVICE_ID = "ALAT-001";
const char* TOPIC_PUB = "altivex/sensor/data";

const uint32_t PUBLISH_INTERVAL_MS = 5000;

constexpr uint8_t GPS_RX_PIN = 16;
constexpr uint8_t GPS_TX_PIN = 17;
constexpr uint32_t GPS_BAUD  = 9600;
constexpr uint8_t VIBRATOR_PIN = 13;

// --- TLS CERTIFICATE ROOT --------------------------------------------
// ISRG Root X1 — sertifikat root Let's Encrypt sejak 2024. Server Anda
// pakai cert dari Let's Encrypt (lewat Caddy / certbot), jadi ESP32
// trust root ini untuk verifikasi server identity.
//
// Kalau Let's Encrypt rotate root cert (jarang, ~10 tahun), update
// nilai di sini dari https://letsencrypt.org/certificates/
//
// Kalau Anda pakai self-signed cert, ganti seluruh blok ini dengan
// public key cert server Anda dalam format PEM.
const char* ROOT_CA = R"EOF(
-----BEGIN CERTIFICATE-----
MIIFazCCA1OgAwIBAgIRAIIQz7DSQONZRGPgu2OCiwAwDQYJKoZIhvcNAQELBQAw
TzELMAkGA1UEBhMCVVMxKTAnBgNVBAoTIEludGVybmV0IFNlY3VyaXR5IFJlc2Vh
cmNoIEdyb3VwMRUwEwYDVQQDEwxJU1JHIFJvb3QgWDEwHhcNMTUwNjA0MTEwNDM4
WhcNMzUwNjA0MTEwNDM4WjBPMQswCQYDVQQGEwJVUzEpMCcGA1UEChMgSW50ZXJu
ZXQgU2VjdXJpdHkgUmVzZWFyY2ggR3JvdXAxFTATBgNVBAMTDElTUkcgUm9vdCBY
MTCCAiIwDQYJKoZIhvcNAQEBBQADggIPADCCAgoCggIBAK3oJHP0FDfzm54rVygc
h77ct984kIxuPOZXoHj3dcKi/vVqbvYATyjb3miGbESTtrFj/RQSa78f0uoxmyF+
0TM8ukj13Xnfs7j/EvEhmkvBioZxaUpmZmyPfjxwv60pIgbz5MDmgK7iS4+3mX6U
A5/TR5d8mUgjU+g4rk8Kb4Mu0UlXjIB0ttov0DiNewNwIRt18jA8+o+u3dpjq+sW
T8KOEUt+zwvo/7V3LvSye0rgTBIlDHCNAymg4VMk7BPZ7hm/ELNKjD+Jo2FR3qyH
B5T0Y3HsLuJvW5iB4YlcNHlsdu87kGJ55tukmi8mxdAQ4Q7e2RCOFvu396j3x+UC
B5iPNgiV5+I3lg02dZ77DnKxHZu8A/lJBdiB3QW0KtZB6awBdpUKD9jf1b0SHzUv
KBds0pjBqAlkd25HN7rOrFleaJ1/ctaJxQZBKT5ZPt0m9STJEadao0xAH0ahmbWn
OlFuhjuefXKnEgV4We0+UXgVCwOPjdAvBbI+e0ocS3MFEvzG6uBQE3xDk3SzynTn
jh8BCNAw1FtxNrQHusEwMFxIt4I7mKZ9YIqioymCzLq9gwQbooMDQaHWBfEbwrbw
qHyGO0aoSCqI3Haadr8faqU9GY/rOPNk3sgrDQoo//fb4hVC1CLQJ13hef4Y53CI
rU7m2Ys6xt0nUW7/vGT1M0NPAgMBAAGjQjBAMA4GA1UdDwEB/wQEAwIBBjAPBgNV
HRMBAf8EBTADAQH/MB0GA1UdDgQWBBR5tFnme7bl5AFzgAiIyBpY9umbbjANBgkq
hkiG9w0BAQsFAAOCAgEAVR9YqbyyqFDQDLHYGmkgJykIrGF1XIpu+ILlaS/V9lZL
ubhzEFnTIZd+50xx+7LSYK05qAvqFyFWhfFQDlnrzuBZ6brJFe+GnY+EgPbk6ZGQ
3BebYhtF8GaV0nxvwuo77x/Py9auJ/GpsMiu/X1+mvoiBOv/2X/qkSsisRcOj/KK
NFtY2PwByVS5uCbMiogziUwthDyC3+6WVwW6LLv3xLfHTjuCvjHIInNzktHCgKQ5
ORAzI4JMPJ+GslWYHb4phowim57iaztXOoJwTdwJx4nLCgdNbOhdjsnvzqvHu7Ur
TkXWStAmzOVyyghqpZXjFaH3pO3JLF+l+/+sKAIuvtd7u+Nxe5AW0wdeRlN8NwdC
jNPElpzVmbUq4JUagEiuTDkHzsxHpFKVK7q4+63SM1N95R1NbdWhscdCb+ZAJzVc
oyi3B43njTOQ5yOf+1CceWxG1bQVs5ZufpsMljq4Ui0/1lvh+wjChP4kqKOJ2qxq
4RgqsahDYVvTH9w7jXbyLeiNdd8XM2w9U/t7y0Ff/9yi0GE44Za4rF2LN9d11TPA
mRGunUHBcnWEvgJBQl9nJEiU0Zsnvgc/ubhPgXRR4Xq37Z0j4r7g1SgEEzwxA57d
emyPxgcYxn/eR44/KJ4EBs+lVDR3veyJm+kXQ99b21/+jh5Xos1AnX5iItreGCc=
-----END CERTIFICATE-----
)EOF";

// --- STATE GLOBAL ----------------------------------------------------
WiFiClientSecure netClient;
PubSubClient     mqtt(netClient);
TinyGPSPlus      gps;
HardwareSerial   gpsSerial(2);

uint32_t lastPublishMs = 0;

// =====================================================================
// connectWifi
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

    // Sync waktu via NTP — TLS validation butuh waktu device akurat
    // (mismatch > 24 jam = sertifikat dianggap invalid).
    Serial.println("[time] Sync NTP ...");
    configTime(0, 0, "pool.ntp.org", "time.google.com");
    time_t now = time(nullptr);
    while (now < 8 * 3600 * 2) {  // wait until > epoch+16h
        delay(500);
        Serial.print(".");
        now = time(nullptr);
    }
    Serial.printf("\n[time] Synced: %s", ctime(&now));
}

// =====================================================================
// connectMqtt — lebih kompleks dibanding plaintext karena harus
// inisialisasi TLS (set CA cert) dan handle exit code spesifik.
// =====================================================================
void connectMqtt() {
    while (!mqtt.connected()) {
        Serial.printf("[mqtt] Connecting (TLS) to %s:%u as '%s' ...\n",
                      MQTT_HOST, MQTT_PORT, MQTT_USERNAME);

        char clientId[40];
        snprintf(clientId, sizeof(clientId), "%s-%llx",
                 DEVICE_ID, ESP.getEfuseMac());

        if (mqtt.connect(clientId, MQTT_USERNAME, MQTT_PASSWORD)) {
            Serial.println("[mqtt] Connected (TLS).");
        } else {
            Serial.printf("[mqtt] Failed (state=%d). Retry in 5s.\n",
                          mqtt.state());
            // State -2 di TLS biasanya = sertifikat tidak valid.
            // Cek ROOT_CA, NTP time, atau hostname mismatch.
            delay(5000);
        }
    }
}

// =====================================================================
// publishPosition
// =====================================================================
void publishPosition(double lat, double lon) {
    StaticJsonDocument<128> doc;
    doc["id_perangkat"] = DEVICE_ID;
    doc["latitude"]  = lat;
    doc["longitude"] = lon;

    char buf[128];
    size_t n = serializeJson(doc, buf, sizeof(buf));

    if (!mqtt.publish(TOPIC_PUB, (const uint8_t*)buf, n, false)) {
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

    // Pasang trust anchor TLS — ESP32 verifikasi server cert lawan
    // ROOT_CA. Kalau setRootCABundle / setCertBundle tidak available
    // di core Anda, ganti `setCACert(ROOT_CA)`.
    netClient.setCACert(ROOT_CA);

    mqtt.setServer(MQTT_HOST, MQTT_PORT);
    mqtt.setKeepAlive(30);
    // Naikkan buffer kalau Anda extend payload (misalnya tambah
    // heart_rate, spo2, battery). Default 256 byte cukup untuk
    // payload sekarang.
    // mqtt.setBufferSize(512);

    connectMqtt();
}

// =====================================================================
// Loop
// =====================================================================
void loop() {
    if (!mqtt.connected()) connectMqtt();
    mqtt.loop();

    while (gpsSerial.available() > 0) {
        gps.encode(gpsSerial.read());
    }

    uint32_t now = millis();
    if (now - lastPublishMs < PUBLISH_INTERVAL_MS) return;
    lastPublishMs = now;

    if (!gps.location.isValid()) {
        Serial.printf("[gps] Waiting for fix... (sat=%lu)\n",
                      gps.satellites.value());
        return;
    }

    double lat = gps.location.lat();
    double lon = gps.location.lng();

    if (fabs(lat) < 1e-6 && fabs(lon) < 1e-6) {
        Serial.println("[gps] Lock-loss anomaly (0,0), skip.");
        return;
    }

    publishPosition(lat, lon);
}

// =====================================================================
// Catatan deployment TLS di server
// ---------------------------------------------------------------------
// Ada dua cara serve MQTT-over-TLS di server ALTIVEX Anda:
//
// CARA 1 — Mosquitto handle TLS langsung (recommended).
//
// 1. Generate cert untuk subdomain MQTT (mis. mqtt.altivex-pangrango...)
//    via certbot DNS-01 atau standalone:
//
//      sudo certbot certonly --standalone \
//          -d mqtt.altivex-pangrango.duckdns.org
//
//    Atau pakai cert yang sama dengan Caddy (kalau pakai SAN cert
//    multi-domain).
//
// 2. Mount cert ke container mosquitto. Edit `docker-compose.yml`:
//
//      mosquitto:
//        ports:
//          - "8883:8883"
//        volumes:
//          - /etc/letsencrypt/live/mqtt.altivex-pangrango.duckdns.org/fullchain.pem:/mosquitto/certs/fullchain.pem:ro
//          - /etc/letsencrypt/live/mqtt.altivex-pangrango.duckdns.org/privkey.pem:/mosquitto/certs/privkey.pem:ro
//          - ./mosquitto/config:/mosquitto/config:ro
//
// 3. Tambah config di `mosquitto/config/mosquitto.conf`:
//
//      listener 8883
//      cafile /mosquitto/certs/fullchain.pem
//      certfile /mosquitto/certs/fullchain.pem
//      keyfile /mosquitto/certs/privkey.pem
//      allow_anonymous false
//      password_file /mosquitto/config/passwd
//
// 4. GCP firewall buka 8883:
//
//      gcloud compute firewall-rules create altivex-mqtts \
//          --allow tcp:8883 --target-tags=mqtt-broker
//
// 5. Restart compose: `docker compose up -d`.
//
// 6. Test dari laptop:
//
//      mosquitto_pub -h mqtt.altivex-pangrango.duckdns.org \
//          -p 8883 --cafile /etc/ssl/certs/ca-certificates.crt \
//          -u altivex_prod -P 'YOUR_PASSWORD' \
//          -t altivex/sensor/data \
//          -m '{"id_perangkat":"TEST","latitude":-6.7711,"longitude":106.96}'
//
// CARA 2 — Reverse stream via nginx Anda yang existing.
//
// nginx Anda bisa terminate TLS di 8883 lalu forward plaintext ke
// container mosquitto:1883 di internal Docker network. Tambah blok di
// /etc/nginx/nginx.conf (DI LUAR `http {}` block, di top level):
//
//   stream {
//       server {
//           listen 8883 ssl;
//           ssl_certificate /etc/letsencrypt/live/altivex-pangrango.duckdns.org/fullchain.pem;
//           ssl_certificate_key /etc/letsencrypt/live/altivex-pangrango.duckdns.org/privkey.pem;
//           proxy_pass 127.0.0.1:1883;
//       }
//   }
//
// Compose Anda harus publish 1883 ke 127.0.0.1 saja (bukan 0.0.0.0):
//
//      mosquitto:
//        ports:
//          - "127.0.0.1:1883:1883"
//
// CARA 1 lebih bersih (mosquitto handle TLS-nya sendiri), tapi
// memerlukan cert di subdomain terpisah. CARA 2 reuse cert utama
// tapi setup-nya lebih banyak moving parts.
// =====================================================================
