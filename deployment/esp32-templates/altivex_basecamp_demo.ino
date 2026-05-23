// =====================================================================
// ALTIVEX BASECAMP DEMO — ESP32 di pos pendakian (Warung Tepi Hutan)
// ---------------------------------------------------------------------
// Fungsi: terima alert otomatis dari server dashboard via MQTT,
// nyalakan buzzer continuous, sampai penjaga tekan tombol acknowledge.
//
// Trigger alert (di-decide otomatis oleh backend):
//   - OUT_OF_GEOFENCE : pendaki keluar koridor jalur
//   - LOW_BATTERY     : baterai pendaki <15%
//   - SIGNAL_LOST     : pendaki status Mendaki >10 menit gak update
//
// State machine basecamp (lokal):
//   - HashSet alert aktif: {(id_perangkat, kind)} → buzzer on/off
//     berdasarkan |set| > 0
//   - Tombol acknowledge fisik: silence buzzer (set silenced=true),
//     TAPI alert tetap di-track
//   - Alert baru masuk setelah silence → re-arm buzzer
//   - Backend kirim cmd "OFF" pas pendaki balik ke koridor / battery
//     pulih / signal pulih → hapus dari set; buzzer mati otomatis
//     kalau set kosong
//
// Library yang perlu di-install via Arduino IDE Library Manager:
//   1. PubSubClient by Nick O'Leary       (>= 2.8)
//   2. ArduinoJson by Benoit Blanchon     (>= 6.21)
//
// Hardware:
//   - ESP32 (board apa saja yang punya WiFi)
//   - Buzzer aktif (active high) di GPIO 13
//   - Tombol push button momentary di GPIO 14 → GND (pakai INPUT_PULLUP)
//   - LED status di GPIO 2 (built-in di mayoritas dev board)
// =====================================================================

#include <WiFi.h>
#include <PubSubClient.h>
#include <ArduinoJson.h>

// ====================================================================
// 1. KONFIGURASI — EDIT 3 BARIS DI BAWAH SEBELUM UPLOAD
// ====================================================================
const char* WIFI_SSID     = "GANTI_SSID_ANDA";
const char* WIFI_PASSWORD = "GANTI_PASSWORD_WIFI";

// MQTT password — ambil dari .env.demo di VM:
//   grep MQTT_PASSWORD ~/ALTIVEX/deployment/demo-branch/.env.demo
const char* MQTT_PASSWORD = "GANTI_DENGAN_MQTT_PASSWORD_DARI_ENV_DEMO";

// ====================================================================
// 2. KONSTANTA BROKER + TOPIC
// ====================================================================
const char*    MQTT_HOST       = "altivex-demo.duckdns.org";
const uint16_t MQTT_PORT       = 1885;
const char*    MQTT_USERNAME   = "altivex_demo";
const char*    BASECAMP_ID     = "BASECAMP-CIFOR";

// Topic yang di-subscribe (input dari backend)
const char*    TOPIC_CMD       = "altivex/basecamp/cmd";
// Topic untuk publish ack (informasi ke backend bahwa penjaga
// sudah tekan tombol; backend hanya log, tidak penting fungsional)
const char*    TOPIC_ACK       = "altivex/basecamp/ack";

// ====================================================================
// 3. HARDWARE PIN
// ====================================================================
const uint8_t BUZZER_PIN       = 13;   // Active high buzzer
const uint8_t ACK_BUTTON_PIN   = 14;   // Tombol momentary push
const uint8_t STATUS_LED_PIN   = 2;    // Built-in LED

// Pola buzzer continuous: 0.5s on / 0.5s off
const uint32_t BUZZ_ON_MS      = 500;
const uint32_t BUZZ_OFF_MS     = 500;

// Debounce tombol
const uint32_t BUTTON_DEBOUNCE_MS = 50;

// Reconnect intervals
const uint32_t WIFI_CHECK_INTERVAL_MS    = 5000;
const uint32_t MQTT_RETRY_DELAY_MS       = 2000;
const uint8_t  MQTT_MAX_RETRIES_PER_LOOP = 3;

// ====================================================================
// 4. STATE GLOBAL
// ====================================================================
WiFiClient    netClient;
PubSubClient  mqtt(netClient);

// Set alert aktif: array kecil untuk efisiensi (max 32 alert simultan
// — lebih dari cukup untuk pos pendakian normal). Format: (id, kind)
// di-encode jadi single string supaya gampang compare.
const uint8_t MAX_ALERTS = 32;
String activeAlerts[MAX_ALERTS];
uint8_t activeAlertCount = 0;

// Penjaga sudah ack? Kalau true, buzzer silent walau set non-empty.
// Reset ke false otomatis kalau alert baru masuk (state ON dengan
// kombinasi (id, kind) yang BELUM ada di set).
bool silenced = false;

// Buzzer state machine
bool     buzzerOn        = false;
uint32_t lastBuzzToggle  = 0;

// Tombol debounce
int      lastButtonState = HIGH;
uint32_t lastButtonChange = 0;

// LED + reconnect
uint32_t lastWifiCheckMs = 0;
uint32_t lastLedToggleMs = 0;
bool     ledState        = false;

// ====================================================================
// Helpers — Wi-Fi + MQTT
// ====================================================================
bool connectWifi(uint32_t timeoutMs = 30000) {
    if (WiFi.status() == WL_CONNECTED) return true;
    Serial.printf("[wifi] Connecting to '%s' ...\n", WIFI_SSID);
    WiFi.mode(WIFI_STA);
    WiFi.disconnect(true);
    delay(100);
    WiFi.begin(WIFI_SSID, WIFI_PASSWORD);
    uint32_t start = millis();
    while (WiFi.status() != WL_CONNECTED) {
        if (millis() - start > timeoutMs) {
            Serial.println("\n[wifi] TIMEOUT.");
            return false;
        }
        delay(500);
        Serial.print(".");
    }
    Serial.printf("\n[wifi] Connected. IP=%s, RSSI=%d dBm\n",
                  WiFi.localIP().toString().c_str(), WiFi.RSSI());
    return true;
}

void onMqttMessage(char* topic, uint8_t* payload, unsigned int length);

bool connectMqtt() {
    if (mqtt.connected()) return true;
    for (uint8_t attempt = 0; attempt < MQTT_MAX_RETRIES_PER_LOOP; attempt++) {
        Serial.printf("[mqtt] Connecting %s:%u as '%s' (attempt %u/%u) ...\n",
                      MQTT_HOST, MQTT_PORT, MQTT_USERNAME,
                      attempt + 1, MQTT_MAX_RETRIES_PER_LOOP);

        char clientId[40];
        uint64_t mac = ESP.getEfuseMac();
        snprintf(clientId, sizeof(clientId), "%s-%08X",
                 BASECAMP_ID, (uint32_t)(mac & 0xFFFFFFFF));

        if (mqtt.connect(clientId, MQTT_USERNAME, MQTT_PASSWORD)) {
            Serial.printf("[mqtt] Connected as '%s'\n", clientId);
            mqtt.subscribe(TOPIC_CMD, /*qos=*/1);
            Serial.printf("[mqtt] Subscribed to %s\n", TOPIC_CMD);
            return true;
        }
        Serial.printf("[mqtt] Failed (state=%d).\n", mqtt.state());
        if (attempt + 1 < MQTT_MAX_RETRIES_PER_LOOP) {
            delay(MQTT_RETRY_DELAY_MS);
        }
    }
    return false;
}

// ====================================================================
// Alert set management
// ====================================================================

/// Cari index alert key di set. Return -1 kalau tidak ada.
int findAlert(const String& key) {
    for (uint8_t i = 0; i < activeAlertCount; i++) {
        if (activeAlerts[i] == key) return i;
    }
    return -1;
}

/// Tambah alert ke set. Return true kalau benar-benar baru
/// (bukan duplicate). Caller pakai return value untuk decide
/// re-arm buzzer atau tidak.
bool addAlert(const String& key) {
    if (findAlert(key) >= 0) return false;
    if (activeAlertCount >= MAX_ALERTS) {
        Serial.printf("⚠️  Set alert penuh (%u), drop: %s\n",
                      activeAlertCount, key.c_str());
        return false;
    }
    activeAlerts[activeAlertCount++] = key;
    Serial.printf("➕ Alert +: %s (total=%u)\n",
                  key.c_str(), activeAlertCount);
    return true;
}

/// Hapus alert dari set. Return true kalau ditemukan & dihapus.
bool removeAlert(const String& key) {
    int idx = findAlert(key);
    if (idx < 0) return false;
    // Shift kiri
    for (uint8_t i = idx; i < activeAlertCount - 1; i++) {
        activeAlerts[i] = activeAlerts[i + 1];
    }
    activeAlertCount--;
    Serial.printf("➖ Alert -: %s (total=%u)\n",
                  key.c_str(), activeAlertCount);
    return true;
}

// ====================================================================
// MQTT message dispatcher
// ====================================================================
void onMqttMessage(char* topic, uint8_t* payload, unsigned int length) {
    Serial.printf("[mqtt] << %s (%u bytes)\n", topic, length);

    StaticJsonDocument<256> doc;
    DeserializationError err = deserializeJson(doc, payload, length);
    if (err) {
        Serial.printf("⚠️  JSON parse error: %s\n", err.c_str());
        return;
    }

    const char* idPerangkat = doc["id_perangkat"] | "";
    const char* kind        = doc["kind"]         | "";
    const char* state       = doc["state"]        | "";
    const char* nama        = doc["nama_pendaki"] | "";
    const char* reason      = doc["reason"]       | "";

    if (strlen(idPerangkat) == 0 || strlen(kind) == 0 || strlen(state) == 0) {
        Serial.println("⚠️  Payload tidak lengkap, skip.");
        return;
    }

    // Construct alert key: "ID|KIND"
    String key = String(idPerangkat) + "|" + String(kind);

    if (strcmp(state, "ON") == 0) {
        bool isNew = addAlert(key);
        Serial.printf("🚨 [%s] %s — %s (%s)\n",
                      kind, idPerangkat, nama, reason);
        // Re-arm: alert baru masuk → cancel silence supaya buzzer
        // menyala lagi. Ini decision dari user (Q3): kalau pendaki
        // sebelumnya keluar trail buang air sebentar, ack 1x silence,
        // tapi kalau dia keluar LAGI, kita mau penjaga tau lagi.
        if (isNew && silenced) {
            silenced = false;
            Serial.println("🔔 Re-arm buzzer (silence dicancel oleh alert baru)");
        }
    } else if (strcmp(state, "OFF") == 0) {
        removeAlert(key);
        Serial.printf("✅ [%s] %s — clear\n", kind, idPerangkat);
        // Kalau set jadi kosong, otomatis reset silenced flag
        // supaya alert berikutnya start fresh.
        if (activeAlertCount == 0) {
            silenced = false;
        }
    } else {
        Serial.printf("⚠️  Unknown state '%s'\n", state);
    }
}

// ====================================================================
// Tombol acknowledge — debounced, edge-triggered
// ====================================================================
void handleAckButton() {
    int reading = digitalRead(ACK_BUTTON_PIN);
    if (reading != lastButtonState) {
        lastButtonChange = millis();
    }
    if ((millis() - lastButtonChange) > BUTTON_DEBOUNCE_MS) {
        // Stable state — kalau LOW (button pressed) dan sebelumnya HIGH,
        // ini falling edge → trigger ack.
        if (reading == LOW && lastButtonState == HIGH) {
            if (activeAlertCount > 0 && !silenced) {
                silenced = true;
                Serial.printf("🔕 ACK button pressed — buzzer silenced "
                              "(set masih %u alert aktif)\n", activeAlertCount);
                // Publish info ke backend (best-effort, gak block)
                if (mqtt.connected()) {
                    char ackPayload[120];
                    snprintf(ackPayload, sizeof(ackPayload),
                             "{\"basecamp\":\"%s\",\"silenced\":true,"
                             "\"active_count\":%u}",
                             BASECAMP_ID, activeAlertCount);
                    mqtt.publish(TOPIC_ACK, ackPayload);
                }
            }
        }
    }
    lastButtonState = reading;
}

// ====================================================================
// Buzzer state machine
// ====================================================================
void updateBuzzer() {
    bool shouldBuzz = (activeAlertCount > 0) && !silenced;

    if (!shouldBuzz) {
        if (buzzerOn) {
            digitalWrite(BUZZER_PIN, LOW);
            buzzerOn = false;
        }
        return;
    }

    // Pulsing pattern: 0.5s on / 0.5s off
    uint32_t now = millis();
    uint32_t targetInterval = buzzerOn ? BUZZ_ON_MS : BUZZ_OFF_MS;
    if (now - lastBuzzToggle >= targetInterval) {
        buzzerOn = !buzzerOn;
        digitalWrite(BUZZER_PIN, buzzerOn ? HIGH : LOW);
        lastBuzzToggle = now;
    }
}

// ====================================================================
// LED status
// ====================================================================
void updateStatusLed() {
    bool wifiOk = (WiFi.status() == WL_CONNECTED);
    bool mqttOk = wifiOk && mqtt.connected();
    if (mqttOk) {
        digitalWrite(STATUS_LED_PIN, HIGH);
    } else if (wifiOk) {
        if (millis() - lastLedToggleMs >= 500) {
            ledState = !ledState;
            digitalWrite(STATUS_LED_PIN, ledState ? HIGH : LOW);
            lastLedToggleMs = millis();
        }
    } else {
        digitalWrite(STATUS_LED_PIN, LOW);
    }
}

// ====================================================================
// Setup
// ====================================================================
void setup() {
    Serial.begin(115200);
    delay(500);

    pinMode(BUZZER_PIN, OUTPUT);
    digitalWrite(BUZZER_PIN, LOW);

    pinMode(ACK_BUTTON_PIN, INPUT_PULLUP);

    pinMode(STATUS_LED_PIN, OUTPUT);
    digitalWrite(STATUS_LED_PIN, LOW);

    Serial.println();
    Serial.println("============================================");
    Serial.println("ALTIVEX BASECAMP DEMO");
    Serial.println("============================================");
    Serial.printf("Basecamp ID:   %s\n", BASECAMP_ID);
    Serial.printf("Broker:        %s:%u\n", MQTT_HOST, MQTT_PORT);
    Serial.printf("Sub topic:     %s\n", TOPIC_CMD);
    Serial.printf("Pub topic:     %s\n", TOPIC_ACK);
    Serial.printf("Buzzer pin:    GPIO%u\n", BUZZER_PIN);
    Serial.printf("Ack button:    GPIO%u (INPUT_PULLUP, active LOW)\n",
                  ACK_BUTTON_PIN);
    Serial.println("============================================");

    connectWifi();
    mqtt.setServer(MQTT_HOST, MQTT_PORT);
    mqtt.setKeepAlive(30);
    mqtt.setBufferSize(512);   // alert payload max ~256, kasih buffer
    mqtt.setCallback(onMqttMessage);
    connectMqtt();

    lastWifiCheckMs = millis();
}

// ====================================================================
// Loop
// ====================================================================
void loop() {
    uint32_t now = millis();

    // 1. Wi-Fi watchdog
    if (now - lastWifiCheckMs >= WIFI_CHECK_INTERVAL_MS) {
        lastWifiCheckMs = now;
        if (WiFi.status() != WL_CONNECTED) {
            Serial.println("[wifi] Disconnected. Reconnecting...");
            connectWifi(10000);
        }
    }

    // 2. MQTT watchdog
    if (WiFi.status() == WL_CONNECTED && !mqtt.connected()) {
        connectMqtt();
    }

    // 3. MQTT message dispatch
    if (mqtt.connected()) {
        mqtt.loop();
    }

    // 4. Tombol ack (debounced)
    handleAckButton();

    // 5. Buzzer state machine
    updateBuzzer();

    // 6. LED status
    updateStatusLed();
}

// =====================================================================
// CARA PAKAI
// ---------------------------------------------------------------------
// 1. Wiring:
//    Buzzer: pin + (positive)  → ESP32 GPIO 13
//            pin - (negative)  → ESP32 GND
//    Button: 1 kaki              → ESP32 GPIO 14
//            kaki seberang       → ESP32 GND
//            (tidak butuh resistor — pakai INPUT_PULLUP internal)
//
// 2. Edit 3 baris di Section 1 (WIFI_SSID, WIFI_PASSWORD, MQTT_PASSWORD)
//
// 3. Compile + upload via Arduino IDE
//
// 4. Buka Serial Monitor 115200. Yang harus muncul:
//
//      ============================================
//      ALTIVEX BASECAMP DEMO
//      ============================================
//      ...
//      [wifi] Connected. IP=192.168.x.x
//      [mqtt] Connected as 'BASECAMP-CIFOR-XXXXXXXX'
//      [mqtt] Subscribed to altivex/basecamp/cmd
//
// 5. Saat pendaki keluar koridor (atau low battery / signal lost),
//    backend auto-publish ke topic tsb. Yang muncul di Serial:
//
//      [mqtt] << altivex/basecamp/cmd (148 bytes)
//      ➕ Alert +: DEMO-CIFOR-01|OUT_OF_GEOFENCE (total=1)
//      🚨 [OUT_OF_GEOFENCE] DEMO-CIFOR-01 — Demo Pendaki 1 (...)
//
//    Buzzer mulai bunyi 0.5s on / 0.5s off.
//
// 6. Penjaga tekan tombol ack:
//
//      🔕 ACK button pressed — buzzer silenced (set masih 1 alert aktif)
//
//    Buzzer berhenti, tapi alert masih di-track.
//
// 7. Pendaki balik ke koridor → backend kirim "OFF":
//
//      [mqtt] << altivex/basecamp/cmd (124 bytes)
//      ➖ Alert -: DEMO-CIFOR-01|OUT_OF_GEOFENCE (total=0)
//      ✅ [OUT_OF_GEOFENCE] DEMO-CIFOR-01 — clear
//
//    Set jadi kosong, silenced flag di-reset, buzzer mati.
//
// ---------------------------------------------------------------------
// SKENARIO DEMO PRESENTASI
// ---------------------------------------------------------------------
//   1. Setup 2 ESP32: pendaki (altivex_demo_situgede.ino) + basecamp
//      (file ini)
//   2. Daftarkan pendaki di dashboard demo dengan ID = DEVICE_ID dari
//      firmware pendaki
//   3. Bawa ESP32 pendaki muter Situgede mengikuti loop CIFOR
//   4. Saat sengaja keluar dari koridor jalan utama, basecamp ESP32
//      di pos buzzing
//   5. Penjaga tekan tombol → silent (demo bahwa false-alarm short-trip
//      "buang air kecil" bisa di-handle penjaga tanpa mengganggu)
//   6. Pendaki balik ke koridor → buzzer mati otomatis (demo bahwa
//      alert auto-clear saat kondisi recover)
//
// ---------------------------------------------------------------------
// TROUBLESHOOTING
// ---------------------------------------------------------------------
//   - Buzzer tidak bunyi padahal Serial print "➕ Alert +"
//        => Cek wiring buzzer. Test manual:
//             digitalWrite(BUZZER_PIN, HIGH);  // pasang di setup()
//           Buzzer harus bunyi terus. Kalau tidak, cek polarity /
//           kabel putus / buzzer rusak.
//
//   - Tombol ack tidak respond
//        => Cek wiring (pin 14 ↔ GND). Pastikan momentary push button
//           (bukan toggle switch). Test dengan multimeter — saat
//           ditekan harus continuity, lepas terbuka.
//
//   - Buzzer terus bunyi walau Serial bilang alert sudah clear
//        => Restart ESP32. Kalau persistent, kemungkinan logic glitch —
//           kasih tau aku.
// =====================================================================
