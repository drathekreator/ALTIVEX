use actix_files::Files;
use actix_web::{web, App, HttpServer, Responder, HttpResponse, HttpRequest, Error};
use actix_web::body::MessageBody;
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::middleware::{from_fn, Next};
use actix_web::web::Payload;
use sqlx::{postgres::PgPoolOptions, Pool, Postgres, FromRow};
use serde::{Deserialize, Serialize};
use dotenvy::dotenv;
use std::env;

use actix::{Actor, StreamHandler, AsyncContext, Handler, Message};
use actix_web_actors::ws;
use tokio::sync::{broadcast, mpsc, oneshot};
use rumqttc::{AsyncClient, MqttOptions, QoS, Event, Packet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use geo::{Contains, Coord, MultiPolygon, Polygon, LineString};
use geojson::{GeoJson, Value};

// ============================================================================
// SerialHub (Task 3.1 — Bug B1, B4)
// ----------------------------------------------------------------------------
// Reader (`start_serial_reader`) dan writer (handler `kirim_peringatan`) WAJIB
// berbagi satu instance port serial agar tidak terjadi konflik akses
// (Windows: "Access Denied" / Linux: "Resource busy") yang sebelumnya membuat
// `kirim_peringatan` jatuh ke fallback "Simulasi" 202.
//
// Pola: port di-pegang `Arc<Mutex<Option<Box<dyn SerialPort + Send>>>>` —
// reader yang me-manage lifecycle (open / reconnect 5s saat absent),
// writer yang dispawn sekali di `start_serial_reader` me-listen ke
// `mpsc::Receiver<SerialCommand>` dan menulis ke port yang sama via
// shared mutex. Handler tidak pernah memanggil `serialport::open()` lagi.
// ============================================================================

/// Perintah serial yang dikirim handler HTTP ke writer task. `payload` sudah
/// jadi byte (handler boleh memformat lewat `format!` di task 3.1, lalu di
/// task 3.2 akan diganti `serde_json::to_vec(&SerialCmd)`). Writer membalas
/// hasil tulis lewat `ack` (`oneshot`), agar handler bisa membalas 200/503.
struct SerialCommand {
    payload: Vec<u8>,
    ack: oneshot::Sender<Result<(), String>>,
}

/// Slot port yang dibagi antara reader & writer task. `None` saat port belum
/// pernah dibuka atau saat sedang offline (alat tidak dicolok / error I/O).
type SharedSerialPort = Arc<Mutex<Option<Box<dyn serialport::SerialPort + Send>>>>;

/// Resource yang diregister sebagai `web::Data<SerialHub>` untuk handler.
/// `tx` jalur perintah ke writer task; `connected` flag bantu observabilitas
/// (status endpoint & logging). Handler tidak perlu mengakses port langsung.
#[derive(Clone)]
struct SerialHub {
    tx: mpsc::Sender<SerialCommand>,
    /// Flag online/offline yang di-update reader. Dipakai task berikutnya
    /// (mis. /api/status yang akurat); disimpan di-Hub agar siap dikonsumsi
    /// handler tanpa refactor lagi.
    #[allow(dead_code)]
    connected: Arc<AtomicBool>,
}

// 1. Model data untuk menerima JSON dari perangkat (transmitter pendaki).
//
// Field `battery` (post-feedback) adalah persen 0-100 yang dihitung di
// firmware transmitter. Optional supaya kompatibel dengan firmware lama
// yang belum kirim battery — null/missing → simpan NULL ke DB →
// frontend render "—".
#[derive(Deserialize, Serialize, Clone)]
struct IncomingData {
    id_perangkat: String,
    latitude: f64,
    longitude: f64,
    /// Persen baterai 0-100 dari transmitter pendaki. `None` = tidak
    /// dikirim (firmware lama). Out-of-range akan di-clamp ke None
    /// di handler supaya nilai gila tidak masuk DB.
    #[serde(default)]
    battery: Option<i16>,
}

// 2. Model data untuk dikirim ke Web Dashboard (diubah ke JSON).
//
// Battery di-include di response /api/sensor + /api/sensor/latest
// supaya frontend bisa render indicator per device.
#[derive(Serialize, FromRow)]
struct SensorRecord {
    id_perangkat: String,
    latitude: f64,
    longitude: f64,
    battery: Option<i16>,
}

// ============================================================================
// Validasi Koordinat (Task 3.3 — Bug B8)
// ----------------------------------------------------------------------------
// NEO-6M kadang lock loss → kirim (0.0, 0.0). Payload jahil/korup juga bisa
// memuat lat/lon di luar range, NaN, atau `id_perangkat` kosong. Tanpa guard,
// row palsu masuk DB & polyline UI melompat ke tengah Atlantik / Teluk Guinea.
//
// Aturan (selaras requirements clause 2.8):
//   - id_perangkat: non-empty setelah trim, panjang ≤ 50 char (sesuai
//     skema kolom `VARCHAR(50)` di tabel `log_sensor` & `pendaki`).
//   - latitude: finite, ∈ [-90.0, 90.0].
//   - longitude: finite, ∈ [-180.0, 180.0].
//   - (latitude, longitude) tidak ≈ (0.0, 0.0) — toleransi ε = 1e-6
//     (kira-kira 0.11 m di equator; jauh lebih kecil dari noise GPS reguler
//     ±2.5 m, jadi tidak akan menolak titik valid di Pulau Null sekalipun).
//
// Fungsi ini WAJIB dipanggil DARI:
//   - HTTP `terima_data` → 400 Bad Request bila gagal.
//   - MQTT branch di `start_mqtt_client` → skip INSERT + log warning.
//   - Serial reader (defense-in-depth, opsional konsisten dengan 3.3).
fn valid_coord(d: &IncomingData) -> bool {
    let id_trimmed = d.id_perangkat.trim();
    let id_ok = !id_trimmed.is_empty() && d.id_perangkat.len() <= 50;
    let lat_ok = d.latitude.is_finite()
        && d.latitude >= -90.0
        && d.latitude <= 90.0;
    let lon_ok = d.longitude.is_finite()
        && d.longitude >= -180.0
        && d.longitude <= 180.0;
    let not_zero = d.latitude.abs() > 1e-6 || d.longitude.abs() > 1e-6;
    id_ok && lat_ok && lon_ok && not_zero
}

/// Sanitize battery value: hanya 0..=100 yang valid. Out-of-range
/// (negatif, >100, atau missing) → None. Firmware bisa kirim 0-100
/// langsung; backend tidak melakukan kalkulasi voltage→persen, itu
/// tanggung jawab transmitter.
fn sanitize_battery(b: Option<i16>) -> Option<i16> {
    match b {
        Some(v) if (0..=100).contains(&v) => Some(v),
        _ => None,
    }
}

// ============================================================================
// Auto-Alert Module — Geofence + Battery + Signal-Lost
// ----------------------------------------------------------------------------
// Backend men-evaluasi 3 kondisi alert tiap publish posisi masuk:
//
//   1. OUT_OF_GEOFENCE — pendaki di luar polygon koridor (50m buffer
//      dari LineString jalur) yang ke-load dari `frontend/GEO.json`.
//   2. LOW_BATTERY     — battery <15% (dilaporkan transmitter pendaki).
//   3. SIGNAL_LOST     — pendaki status='Mendaki' tapi >10 menit tidak
//      ada publish baru. Di-detect periodic task tiap 30 detik.
//
// State machine alert per (id_perangkat, kategori):
//   Inactive  -> Active   : trigger publish ke MQTT topic basecamp +
//                           backend memori `active_alerts.insert(...)`.
//   Active    -> Active   : NO-OP (debounce — gak spam basecamp).
//   Active    -> Inactive : trigger publish CLEAR ke basecamp +
//                           `active_alerts.remove(...)`.
//
// Basecamp ESP32 subscribe ke `altivex/basecamp/cmd` dan maintain
// HashSet alert aktif sendiri. Buzzer continuous selama set non-empty.
// Tombol acknowledge di basecamp kirim back via topic `altivex/
// basecamp/ack` (dipakai backend untuk reset notification flag tapi
// alert tetap "active" sampai kondisi clear).
// ============================================================================

/// Kategori alert untuk komunikasi backend ↔ basecamp ESP32.
/// Stringified ke JSON: "OUT_OF_GEOFENCE" / "LOW_BATTERY" / "SIGNAL_LOST".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum AlertKind {
    OutOfGeofence,
    LowBattery,
    SignalLost,
}

impl AlertKind {
    fn as_str(self) -> &'static str {
        match self {
            AlertKind::OutOfGeofence => "OUT_OF_GEOFENCE",
            AlertKind::LowBattery    => "LOW_BATTERY",
            AlertKind::SignalLost    => "SIGNAL_LOST",
        }
    }
}

/// Payload yang di-publish backend ke `altivex/basecamp/cmd`.
/// `state`: "ON"  = trigger alert (tambah ke set basecamp).
///          "OFF" = clear alert (hapus dari set basecamp).
#[derive(Debug, Serialize)]
struct BasecampCmd<'a> {
    id_perangkat: &'a str,
    nama_pendaki: Option<&'a str>,
    kind: &'static str,
    state: &'static str,
    reason: String,
}

/// State observability per pendaki yang dipakai signal-lost detector.
/// `last_seen` di-update tiap publish posisi masuk; periodic task
/// scan map ini, kalau ada entry yang `last_seen` lebih dari 10 menit
/// lalu DAN pendaki masih status='Mendaki' di DB → trigger SIGNAL_LOST.
///
/// Field `last_lat/last_lon/last_battery` di-track untuk future use
/// (mis. operator query "posisi terakhir sebelum signal lost"); saat
/// ini cuma `last_seen` yang dipakai watcher.
#[derive(Debug, Clone)]
struct DeviceObservability {
    last_seen: Instant,
    #[allow(dead_code)]
    last_lat: f64,
    #[allow(dead_code)]
    last_lon: f64,
    #[allow(dead_code)]
    last_battery: Option<i16>,
}

/// Hub state alert — dishare antara MQTT handler, periodic task,
/// dan publisher basecamp via Arc<Mutex<...>>.
struct AlertHub {
    /// Polygon koridor (hasil load + buffer dari GEO.json).
    /// `None` saat GEO.json gagal load (fail-open: alert geofence
    /// di-skip, tapi alert lain tetap jalan).
    geofence: Option<MultiPolygon<f64>>,
    /// Set alert aktif: (id_perangkat, kind) → tracking transition
    /// supaya tidak spam basecamp.
    active: HashSet<(String, AlertKind)>,
    /// Last-seen tracker per device — input untuk signal-lost detector.
    devices: HashMap<String, DeviceObservability>,
    /// MQTT client untuk publish ke basecamp. Set `Some` setelah
    /// MQTT subscriber connect ke broker.
    mqtt: Option<AsyncClient>,
}

impl AlertHub {
    fn new(geofence: Option<MultiPolygon<f64>>) -> Self {
        Self {
            geofence,
            active: HashSet::new(),
            devices: HashMap::new(),
            mqtt: None,
        }
    }
}

/// Threshold konfigurasi alert. Kalau ke depan mau di-tune lewat
/// env, tinggal ganti hard-coded ini ke `env::var(...)`.
const SIGNAL_LOST_THRESHOLD: Duration = Duration::from_secs(10 * 60); // 10 menit
const LOW_BATTERY_THRESHOLD_PCT: i16 = 15;
const SIGNAL_LOST_CHECK_INTERVAL: Duration = Duration::from_secs(30);
const GEOFENCE_BUFFER_DEG: f64 = 0.00045; // ≈ 50m di equator

/// Topic MQTT untuk push alert ke basecamp ESP32.
/// Basecamp subscribe ke topic ini, maintain HashSet alert lokal,
/// buzzer continuous selama set non-empty.
const TOPIC_BASECAMP_CMD: &str = "altivex/basecamp/cmd";

/// Load GeoJSON dari path, ekstrak semua LineString feature dengan
/// property `type=route`, build MultiPolygon buffer (~50m) sebagai
/// koridor geofence.
///
/// Return `None` kalau:
///   - File tidak ada / unreadable
///   - JSON malformed
///   - Tidak ada feature route ditemukan
///
/// Caller: `main()` saat startup, hasilnya di-share via Arc<Mutex<AlertHub>>.
/// Fail-open by design — kalau load gagal, alert geofence di-skip,
/// tapi battery + signal-lost tetap jalan.
fn load_geofence(path: &str) -> Option<MultiPolygon<f64>> {
    let content = std::fs::read_to_string(path).ok()?;
    let geo: GeoJson = content.parse().ok()?;
    let collection = match geo {
        GeoJson::FeatureCollection(fc) => fc,
        _ => return None,
    };

    let mut polygons: Vec<Polygon<f64>> = Vec::new();

    for feature in collection.features {
        let Some(geom) = feature.geometry else { continue };
        let line: LineString<f64> = match geom.value {
            Value::LineString(coords) => coords
                .into_iter()
                .filter_map(|c| {
                    if c.len() >= 2 {
                        Some(Coord { x: c[0], y: c[1] })
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .into(),
            Value::Polygon(rings) => {
                // Polygon manual sudah ada di GEO.json (mis. kasus
                // user-defined geofence_corridor). Pakai langsung
                // tanpa buffering.
                if let Some(outer) = rings.first() {
                    let coords: Vec<Coord<f64>> = outer
                        .iter()
                        .filter_map(|c| {
                            if c.len() >= 2 {
                                Some(Coord { x: c[0], y: c[1] })
                            } else {
                                None
                            }
                        })
                        .collect();
                    if coords.len() >= 4 {
                        polygons.push(Polygon::new(LineString(coords), vec![]));
                    }
                }
                continue;
            }
            _ => continue,
        };
        if line.0.len() < 2 {
            continue;
        }
        // Buffer manual: untuk tiap segment, bikin rectangle melebar
        // 50m kiri-kanan. Cara ini lebih ringan daripada `geo-buffer`
        // crate dependency tambahan.
        polygons.extend(buffer_linestring(&line, GEOFENCE_BUFFER_DEG));
    }

    if polygons.is_empty() {
        return None;
    }
    Some(MultiPolygon(polygons))
}

/// Bikin rectangle sederhana di sekeliling tiap segment LineString,
/// lebar `buffer_deg` derajat (≈ buffer_deg * 111km). Hasilnya
/// vec polygon yang nanti di-union jadi MultiPolygon — `Contains`
/// di geo crate akan return true kalau point di salah satu polygon.
fn buffer_linestring(line: &LineString<f64>, buffer_deg: f64) -> Vec<Polygon<f64>> {
    let coords: Vec<&Coord<f64>> = line.0.iter().collect();
    let mut polys = Vec::new();
    for window in coords.windows(2) {
        let a = window[0];
        let b = window[1];
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-9 {
            continue;
        }
        // Vektor tegak lurus (di-normalize) × buffer
        let nx = -dy / len * buffer_deg;
        let ny = dx / len * buffer_deg;
        let p1 = Coord { x: a.x + nx, y: a.y + ny };
        let p2 = Coord { x: b.x + nx, y: b.y + ny };
        let p3 = Coord { x: b.x - nx, y: b.y - ny };
        let p4 = Coord { x: a.x - nx, y: a.y - ny };
        let exterior = LineString(vec![p1, p2, p3, p4, p1]);
        polys.push(Polygon::new(exterior, vec![]));
    }
    polys
}

/// True kalau (lat, lon) di dalam polygon koridor.
/// Fail-open: kalau geofence belum di-load (None), return true
/// supaya alert geofence dianggap "always inside" — UI tidak
/// trigger banner palsu, basecamp tidak buzzer palsu.
fn point_in_geofence(geofence: Option<&MultiPolygon<f64>>, lat: f64, lon: f64) -> bool {
    match geofence {
        Some(mp) => mp.contains(&geo::Point::new(lon, lat)),
        None => true,
    }
}

/// Helper: lookup nama_pendaki dari id_perangkat untuk pesan alert
/// yang lebih informatif. Best-effort — kalau DB error atau tidak
/// ditemukan, return None.
async fn lookup_nama_pendaki(
    pool: &Pool<Postgres>,
    id_perangkat: &str,
) -> Option<String> {
    let result: Result<Option<(String,)>, _> = sqlx::query_as(
        "SELECT nama_pendaki FROM pendaki \
         WHERE id_perangkat = $1 AND status = 'Mendaki' \
         ORDER BY tanggal_naik DESC LIMIT 1",
    )
    .bind(id_perangkat)
    .fetch_optional(pool)
    .await;
    result.ok().flatten().map(|(n,)| n)
}

/// Push alert ke basecamp ESP32 via MQTT publish ke
/// `altivex/basecamp/cmd`. Idempotent dengan state machine:
/// caller bertanggung jawab cek `active.insert/remove` sebelum publish
/// supaya tidak spam.
async fn publish_basecamp_cmd(
    mqtt: &AsyncClient,
    cmd: &BasecampCmd<'_>,
) {
    let payload = match serde_json::to_vec(cmd) {
        Ok(v) => v,
        Err(e) => {
            println!("⚠️  Gagal serialize BasecampCmd: {}", e);
            return;
        }
    };
    if let Err(e) = mqtt
        .publish(TOPIC_BASECAMP_CMD, QoS::AtLeastOnce, false, payload)
        .await
    {
        println!(
            "⚠️  Gagal publish basecamp cmd ({} {}): {:?}",
            cmd.kind, cmd.id_perangkat, e
        );
    } else {
        println!(
            "🚨 Basecamp cmd → {} {} (state={}) reason='{}'",
            cmd.id_perangkat, cmd.kind, cmd.state, cmd.reason
        );
    }
}

/// Evaluate semua kondisi alert (geofence + battery) untuk satu
/// publish posisi yang baru masuk, fire transition ON/OFF ke basecamp
/// kalau state berubah.
///
/// SIGNAL_LOST tidak di-evaluasi di sini — itu dihandle periodic task
/// tiap 30 detik (lihat `start_signal_lost_watcher`). Tapi `last_seen`
/// di update di sini supaya watcher punya data fresh.
async fn evaluate_alerts(
    hub: &Arc<Mutex<AlertHub>>,
    pool: &Pool<Postgres>,
    data: &IncomingData,
) {
    // 1. Update last-seen + lookup nama_pendaki sebelum lock hub.
    let nama = lookup_nama_pendaki(pool, &data.id_perangkat).await;
    let battery = sanitize_battery(data.battery);

    // 2. Hitung outcome di scope yang me-lock hub minimal.
    let (transitions, mqtt_opt) = {
        let mut h = hub.lock().unwrap();
        h.devices.insert(
            data.id_perangkat.clone(),
            DeviceObservability {
                last_seen: Instant::now(),
                last_lat: data.latitude,
                last_lon: data.longitude,
                last_battery: battery,
            },
        );

        let mut transitions: Vec<(AlertKind, bool, String)> = Vec::new();

        // OUT_OF_GEOFENCE
        let inside = point_in_geofence(h.geofence.as_ref(), data.latitude, data.longitude);
        let key_geo = (data.id_perangkat.clone(), AlertKind::OutOfGeofence);
        let was_active_geo = h.active.contains(&key_geo);
        if !inside && !was_active_geo {
            h.active.insert(key_geo.clone());
            transitions.push((
                AlertKind::OutOfGeofence,
                true,
                format!("Posisi ({:.5}, {:.5}) di luar koridor", data.latitude, data.longitude),
            ));
        } else if inside && was_active_geo {
            h.active.remove(&key_geo);
            transitions.push((AlertKind::OutOfGeofence, false, "Kembali ke koridor".into()));
        }

        // LOW_BATTERY
        let key_bat = (data.id_perangkat.clone(), AlertKind::LowBattery);
        let was_active_bat = h.active.contains(&key_bat);
        match battery {
            Some(b) if b < LOW_BATTERY_THRESHOLD_PCT && !was_active_bat => {
                h.active.insert(key_bat.clone());
                transitions.push((AlertKind::LowBattery, true, format!("Baterai {}%", b)));
            }
            Some(b) if b >= LOW_BATTERY_THRESHOLD_PCT && was_active_bat => {
                h.active.remove(&key_bat);
                transitions.push((AlertKind::LowBattery, false, format!("Baterai pulih {}%", b)));
            }
            _ => {}
        }

        // SIGNAL_LOST clear-on-update: kalau publish baru masuk dari
        // device yang sebelumnya signal-lost, otomatis OFF.
        let key_sig = (data.id_perangkat.clone(), AlertKind::SignalLost);
        if h.active.contains(&key_sig) {
            h.active.remove(&key_sig);
            transitions.push((AlertKind::SignalLost, false, "Sinyal pulih".into()));
        }

        (transitions, h.mqtt.clone())
    };

    // 3. Publish ke MQTT di luar lock supaya gak block handler lain.
    if let Some(mqtt) = mqtt_opt {
        for (kind, on, reason) in transitions {
            let cmd = BasecampCmd {
                id_perangkat: &data.id_perangkat,
                nama_pendaki: nama.as_deref(),
                kind: kind.as_str(),
                state: if on { "ON" } else { "OFF" },
                reason,
            };
            publish_basecamp_cmd(&mqtt, &cmd).await;
        }
    }
}

/// Periodic task — scan `hub.devices`, kalau ada entry dengan
/// `last_seen` lebih lama dari 10 menit DAN pendaki masih
/// status='Mendaki' di DB, trigger SIGNAL_LOST. Idempotent (cuma
/// trigger ON sekali per transition).
async fn start_signal_lost_watcher(
    hub: Arc<Mutex<AlertHub>>,
    pool: Pool<Postgres>,
) {
    println!(
        "👀 Signal-lost watcher aktif (threshold={}s, interval={}s)",
        SIGNAL_LOST_THRESHOLD.as_secs(),
        SIGNAL_LOST_CHECK_INTERVAL.as_secs()
    );
    let mut ticker = tokio::time::interval(SIGNAL_LOST_CHECK_INTERVAL);
    // Skip first tick (immediate fire bikin false-positive saat startup).
    ticker.tick().await;
    loop {
        ticker.tick().await;

        // Snapshot device yang LAMA tidak terlihat (release lock cepat).
        let candidates: Vec<(String, DeviceObservability)> = {
            let h = hub.lock().unwrap();
            let now = Instant::now();
            h.devices
                .iter()
                .filter(|(id, obs)| {
                    let key = (id.to_string(), AlertKind::SignalLost);
                    let timeout = now.duration_since(obs.last_seen) >= SIGNAL_LOST_THRESHOLD;
                    timeout && !h.active.contains(&key)
                })
                .map(|(id, obs)| (id.clone(), obs.clone()))
                .collect()
        };

        for (id, _obs) in candidates {
            // Konfirmasi pendaki masih status='Mendaki' (kalau sudah
            // turun, signal-lost tidak relevan).
            let still_active: Result<Option<(i64,)>, _> = sqlx::query_as(
                "SELECT COUNT(*) FROM pendaki \
                 WHERE id_perangkat = $1 AND status = 'Mendaki'",
            )
            .bind(&id)
            .fetch_optional(&pool)
            .await;
            let still_active = matches!(still_active, Ok(Some((n,))) if n > 0);
            if !still_active {
                continue;
            }

            // Lookup nama untuk pesan + lock hub minimal.
            let nama = lookup_nama_pendaki(&pool, &id).await;
            let mqtt_opt = {
                let mut h = hub.lock().unwrap();
                let key = (id.clone(), AlertKind::SignalLost);
                if h.active.contains(&key) {
                    None
                } else {
                    h.active.insert(key);
                    h.mqtt.clone()
                }
            };
            if let Some(mqtt) = mqtt_opt {
                let cmd = BasecampCmd {
                    id_perangkat: &id,
                    nama_pendaki: nama.as_deref(),
                    kind: AlertKind::SignalLost.as_str(),
                    state: "ON",
                    reason: format!(
                        "Tidak ada sinyal >= {} menit",
                        SIGNAL_LOST_THRESHOLD.as_secs() / 60
                    ),
                };
                publish_basecamp_cmd(&mqtt, &cmd).await;
            }
        }
    }
}

// 3. Endpoint POST: Menyimpan data baru ke Database
async fn terima_data(
    data: web::Json<IncomingData>,
    pool: web::Data<Pool<Postgres>>,
    tx: web::Data<broadcast::Sender<String>>,
    hub: web::Data<Arc<Mutex<AlertHub>>>,
) -> impl Responder {
    // Task 3.3 — guard koordinat & ID perangkat sebelum menyentuh DB.
    // Payload jahil (lat/lon out-of-range, NaN, id kosong, atau (0,0) dari
    // NEO-6M lock loss) ditolak dengan 400 Bad Request, sehingga `log_sensor`
    // tidak terkontaminasi dan WS broadcast tidak menyebarkan titik palsu.
    if !valid_coord(&data) {
        println!(
            "⚠️  Payload sensor ditolak (HTTP /api/sensor) — id='{}', lat={}, lon={}",
            data.id_perangkat, data.latitude, data.longitude
        );
        return HttpResponse::BadRequest()
            .body("Payload tidak valid: id_perangkat / latitude / longitude di luar batas yang diizinkan.");
    }

    let query = "
        INSERT INTO log_sensor (id_perangkat, latitude, longitude, battery)
        VALUES ($1, $2, $3, $4)
    ";

    let battery = sanitize_battery(data.battery);

    // Mengeksekusi query insert ke PostgreSQL
    let result = sqlx::query(query)
        .bind(&data.id_perangkat)
        .bind(data.latitude)
        .bind(data.longitude)
        .bind(battery)
        .execute(pool.get_ref())
        .await;

    // Broadcast data ke semua client WebSocket yang terhubung
    if let Ok(json_str) = serde_json::to_string(&*data) {
        let _ = tx.send(json_str);
    }

    // Auto-alert evaluation (geofence + battery transitions, async).
    evaluate_alerts(hub.get_ref(), pool.get_ref(), &data).await;

    match result {
        Ok(_) => HttpResponse::Ok().body("Berhasil: Data sensor tersimpan di Database!"),
        Err(e) => HttpResponse::InternalServerError().body(format!("Gagal menyimpan: {}", e)),
    }
}

// 4. Endpoint GET: Mengambil data terbaru untuk ditampilkan di Peta
async fn ambil_data(pool: web::Data<Pool<Postgres>>) -> impl Responder {
    // Menarik 50 data terbaru
    let query = "SELECT id_perangkat, latitude, longitude, battery FROM log_sensor ORDER BY timestamp DESC LIMIT 50";

    let records = sqlx::query_as::<_, SensorRecord>(query)
        .fetch_all(pool.get_ref())
        .await;

    match records {
        Ok(data) => HttpResponse::Ok().json(data),
        Err(e) => HttpResponse::InternalServerError().body(format!("Gagal mengambil data: {}", e)),
    }
}

// ============================================================================
// Endpoint Baru `GET /api/sensor/latest` (Task 3.6 — Bug F4)
// ----------------------------------------------------------------------------
// Endpoint lama `GET /api/sensor` memakai `LIMIT 50` baris terbaru lintas
// alat — saat ≥6 alat aktif streaming dengan frekuensi tinggi, 50 baris itu
// bisa habis untuk 1–2 alat saja sehingga alat lain tidak muncul di sidebar
// sampai mereka push WS lagi (requirement 1.14).
//
// Fix: tarik tepat 1 baris terbaru per `id_perangkat` lewat
// `DISTINCT ON (id_perangkat) ... ORDER BY id_perangkat, timestamp DESC`
// (Postgres-specific). Sidebar/peta SHALL menampilkan semua alat aktif tanpa
// terkapped (requirement 2.14). Endpoint lama TETAP DIPERTAHANKAN apa adanya
// untuk preserve clause 3.4 — frontend yang akan switch ke endpoint baru.
//
// Response shape identik dengan `ambil_data` (`Vec<SensorRecord>`) sehingga
// frontend cukup mengganti URL fetch tanpa perlu mengubah parser/render.
// ============================================================================
async fn ambil_sensor_latest(pool: web::Data<Pool<Postgres>>) -> impl Responder {
    // `DISTINCT ON (id_perangkat)` memilih baris pertama (sesuai ORDER BY)
    // per alat. Karena `ORDER BY id_perangkat, timestamp DESC`, "pertama"
    // di sini = baris dengan timestamp paling baru untuk tiap alat — yaitu
    // posisi terkini per alat, persis yang dibutuhkan sidebar.
    let query = "SELECT DISTINCT ON (id_perangkat) id_perangkat, latitude, longitude, battery \
                 FROM log_sensor \
                 ORDER BY id_perangkat, timestamp DESC";

    let records = sqlx::query_as::<_, SensorRecord>(query)
        .fetch_all(pool.get_ref())
        .await;

    match records {
        Ok(data) => HttpResponse::Ok().json(data),
        Err(e) => HttpResponse::InternalServerError()
            .body(format!("Gagal mengambil data terbaru: {}", e)),
    }
}

// Model data untuk History Path (Hanya Koordinat)
#[derive(Serialize, FromRow)]
struct HistoryRecord {
    latitude: f64,
    longitude: f64,
}

// ============================================================================
// History Window-Aware (Task 3.5 — Bug B9)
// ----------------------------------------------------------------------------
// Sebelum fix, `ambil_history` mengembalikan SELURUH koordinat sepanjang
// sejarah `id_perangkat`. Akibatnya: alat yang dipakai pendaki1 lalu
// pendaki2 punya polyline yang menyatu — jalur pendaki2 nyambung ke jalur
// kemarin (Bug B9 / requirement 2.9).
//
// Strategi fix:
//   1. `GET /api/history/{id_perangkat}` = LIVE route untuk pendaki yang
//      SEDANG mendaki. Filter berdasarkan window pendakian AKTIF saja
//      (status='Mendaki'). Saat tidak ada pendakian aktif → result empty
//      (subquery NULL → predikat `timestamp >= NULL` evaluasi NULL →
//      row tidak di-include).
//   2. `GET /api/pendaki/{id}/history` = endpoint baru untuk pendaki
//      tertentu (boleh sudah turun). Filter window
//      `[tanggal_naik, COALESCE(tanggal_turun, CURRENT_TIMESTAMP)]`.

// Endpoint GET /api/history/{id_perangkat} — riwayat alat (window aktif).
//
// Filter: hanya koordinat sejak `tanggal_naik` pendakian aktif terbaru
// untuk alat tsb. Jika alat tidak punya pendakian aktif (semua sudah
// turun / belum pernah dipakai), subquery mengembalikan NULL dan hasilnya
// empty array — itulah perilaku yang benar (tidak ada pendakian "live"
// untuk alat tsb).
async fn ambil_history(
    path: web::Path<String>,
    pool: web::Data<Pool<Postgres>>,
) -> impl Responder {
    let id_perangkat = path.into_inner();
    // SQL window-aware: timestamp >= tanggal_naik pendakian aktif terbaru.
    // - Subquery NULL (tidak ada pendakian aktif) → predikat NULL → empty.
    // - Subquery ada → ambil koordinat sejak naik, urut menaik (untuk
    //   polyline kronologis di frontend).
    let query = "
        SELECT latitude, longitude
        FROM log_sensor
        WHERE id_perangkat = $1
          AND timestamp >= (
            SELECT tanggal_naik FROM pendaki
            WHERE id_perangkat = $1 AND status = 'Mendaki'
            ORDER BY tanggal_naik DESC LIMIT 1
          )
        ORDER BY timestamp ASC
    ";

    let records = sqlx::query_as::<_, HistoryRecord>(query)
        .bind(&id_perangkat)
        .fetch_all(pool.get_ref())
        .await;

    match records {
        Ok(data) => HttpResponse::Ok().json(data),
        Err(e) => HttpResponse::InternalServerError().body(format!("Gagal mengambil history: {}", e)),
    }
}

// Endpoint GET /api/pendaki/{id}/history — riwayat pendaki spesifik.
//
// Untuk pendaki yang SUDAH TURUN, frontend perlu menampilkan polyline
// pendakian-nya sendiri tanpa nyambung ke pendaki lain di alat yang sama.
// Kita resolve `id_perangkat`, `tanggal_naik`, `tanggal_turun` dari baris
// pendaki, lalu filter `log_sensor` di window
// `[tanggal_naik, COALESCE(tanggal_turun, CURRENT_TIMESTAMP)]`.
//
// Untuk pendaki yang masih `Mendaki` (`tanggal_turun IS NULL`), window
// di-cap ke `CURRENT_TIMESTAMP` — hasilnya sama dengan endpoint live
// di atas (preserve sinyal real-time).
//
// Path param: `id` (PK integer pendaki). Pendaki tidak ditemukan → 404.
async fn ambil_history_pendaki(
    path: web::Path<i32>,
    pool: web::Data<Pool<Postgres>>,
) -> impl Responder {
    let id = path.into_inner();
    // CTE `p` mengambil baris pendaki tunggal; query utama JOIN ke
    // log_sensor pakai id_perangkat-nya. Window di-cap ke
    // `tanggal_turun` jika ada, atau `CURRENT_TIMESTAMP` jika belum
    // turun (`Option<NaiveDateTime>` → NULL di PG).
    let query = "
        WITH p AS (
            SELECT id_perangkat, tanggal_naik, tanggal_turun
            FROM pendaki WHERE id = $1
        )
        SELECT ls.latitude, ls.longitude
        FROM log_sensor ls
        JOIN p ON ls.id_perangkat = p.id_perangkat
        WHERE ls.timestamp >= p.tanggal_naik
          AND ls.timestamp <= COALESCE(p.tanggal_turun, CURRENT_TIMESTAMP)
        ORDER BY ls.timestamp ASC
    ";

    // Cek dulu apakah pendaki ada (preserve perilaku 3.6 — struktur
    // respons; di sini kita pisahkan 404 vs 200 array kosong).
    let exists: Result<(i64,), _> = sqlx::query_as(
        "SELECT COUNT(*) FROM pendaki WHERE id = $1"
    )
    .bind(id)
    .fetch_one(pool.get_ref())
    .await;

    match exists {
        Ok((0,)) => return HttpResponse::NotFound().body("Pendaki tidak ditemukan."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("Error: {}", e)),
        _ => {}
    }

    let records = sqlx::query_as::<_, HistoryRecord>(query)
        .bind(id)
        .fetch_all(pool.get_ref())
        .await;

    match records {
        Ok(data) => HttpResponse::Ok().json(data),
        Err(e) => HttpResponse::InternalServerError().body(format!("Gagal mengambil history: {}", e)),
    }
}

// Model data & Endpoint untuk Manajemen Pendaki (CRUD)
//
// Task 3.5 (Bug B9) menambahkan kolom `tanggal_turun: Option<NaiveDateTime>`
// untuk men-define window pendakian `[tanggal_naik, tanggal_turun]`.
// Default-nya `None` (pendaki masih `Mendaki`); diisi saat
// `selesaikan_pendakian` dipanggil.
//
// User feedback (post-deploy): operator basecamp butuh visibility
// `tanggal_turun` di tabel riwayat & export Excel untuk audit waktu
// turun pendaki yang sudah selesai. Sebelumnya field ini di-skip dari
// JSON response — sekarang kita expose. `Option<NaiveDateTime>` akan
// di-serialize sebagai `null` saat pendaki masih mendaki, dan
// timestamp ISO-8601 saat sudah turun.
//
// Field tetap dapat di-load via `FromRow` (sqlx tidak melihat
// atribut serde), sehingga query `SELECT * FROM pendaki ...` tetap
// bekerja setelah migrasi `ADD COLUMN IF NOT EXISTS tanggal_turun`.
#[derive(Serialize, FromRow)]
struct Pendaki {
    id: i32,
    nama_pendaki: String,
    id_perangkat: String,
    telepon_darurat: String,
    tanggal_naik: chrono::NaiveDateTime,
    status: String,
    /// Set saat status diubah ke 'Sudah Turun' (lihat
    /// `selesaikan_pendakian`). Untuk pendaki aktif, field ini `None`
    /// (di-serialize sebagai JSON `null`). Dipakai endpoint
    /// `/api/pendaki/{id}/history` untuk men-cap window pencarian
    /// koordinat, DAN sekarang juga di-render di tabel riwayat +
    /// Export Excel untuk operator basecamp.
    tanggal_turun: Option<chrono::NaiveDateTime>,
}

#[derive(Deserialize)]
struct RegistrasiPendaki {
    nama_pendaki: String,
    id_perangkat: String,
    telepon_darurat: String,
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
}

// GET /api/pendaki
async fn ambil_pendaki(pool: web::Data<Pool<Postgres>>) -> impl Responder {
    let query = "SELECT * FROM pendaki WHERE status = 'Mendaki' ORDER BY tanggal_naik DESC";
    let records = sqlx::query_as::<_, Pendaki>(query).fetch_all(pool.get_ref()).await;
    match records {
        Ok(data) => HttpResponse::Ok().json(data),
        Err(e) => HttpResponse::InternalServerError().body(format!("Error: {}", e)),
    }
}

// POST /api/pendaki
async fn registrasi_pendaki(data: web::Json<RegistrasiPendaki>, pool: web::Data<Pool<Postgres>>) -> impl Responder {
    let query = "INSERT INTO pendaki (nama_pendaki, id_perangkat, telepon_darurat, status) VALUES ($1, $2, $3, 'Mendaki')";
    let result = sqlx::query(query)
        .bind(&data.nama_pendaki)
        .bind(&data.id_perangkat)
        .bind(&data.telepon_darurat)
        .execute(pool.get_ref())
        .await;
    match result {
        Ok(_) => HttpResponse::Ok().body("Pendaki berhasil diregistrasi."),
        Err(e) => HttpResponse::InternalServerError().body(format!("Gagal mendaftar: {}", e)),
    }
}

// PUT /api/pendaki/{id_perangkat}/selesai
//
// Task 3.4 — Cek `rows_affected()` (Bug B7): tanpa cek ini, request untuk
// `id_perangkat` yang tidak match baris mana pun (mis. alat yang sudah
// turun atau tidak terdaftar) tetap dibalas `200 OK`. Sekarang `0 row`
// → `404 Not Found`, `>=1` → `200 OK`, error DB → `500`.
//
// Task 3.5 — set `tanggal_turun = CURRENT_TIMESTAMP` saat status diubah
// ke 'Sudah Turun'. Nilai ini menjadi batas atas window pencarian
// koordinat di endpoint `/api/pendaki/{id}/history`, sehingga polyline
// pendaki yang sudah turun TIDAK menyatu dengan jalur pendaki berikutnya
// yang memakai alat yang sama (Bug B9).
async fn selesaikan_pendakian(path: web::Path<String>, pool: web::Data<Pool<Postgres>>) -> impl Responder {
    let id_perangkat = path.into_inner();
    let query = "UPDATE pendaki SET status = 'Sudah Turun', tanggal_turun = CURRENT_TIMESTAMP WHERE id_perangkat = $1 AND status = 'Mendaki'";
    let result = sqlx::query(query).bind(&id_perangkat).execute(pool.get_ref()).await;
    match result {
        Ok(res) => {
            if res.rows_affected() == 0 {
                HttpResponse::NotFound().body("Pendaki tidak ditemukan.")
            } else {
                HttpResponse::Ok().body("Pendakian diselesaikan.")
            }
        }
        Err(e) => HttpResponse::InternalServerError().body(format!("Error: {}", e)),
    }
}

// GET /api/pendaki/riwayat — Ambil semua pendaki (termasuk yang sudah turun)
async fn ambil_riwayat_pendaki(pool: web::Data<Pool<Postgres>>) -> impl Responder {
    let query = "SELECT * FROM pendaki ORDER BY tanggal_naik DESC LIMIT 100";
    let records = sqlx::query_as::<_, Pendaki>(query).fetch_all(pool.get_ref()).await;
    match records {
        Ok(data) => HttpResponse::Ok().json(data),
        Err(e) => HttpResponse::InternalServerError().body(format!("Error: {}", e)),
    }
}

// DELETE /api/pendaki/{id} — Hapus data pendaki
//
// Task 3.4 — sama seperti `selesaikan_pendakian`: 0 row → 404, >=1 → 200,
// error DB → 500.
async fn hapus_pendaki(path: web::Path<i32>, pool: web::Data<Pool<Postgres>>) -> impl Responder {
    let id = path.into_inner();
    let result = sqlx::query("DELETE FROM pendaki WHERE id = $1").bind(id).execute(pool.get_ref()).await;
    match result {
        Ok(res) => {
            if res.rows_affected() == 0 {
                HttpResponse::NotFound().body("Pendaki tidak ditemukan.")
            } else {
                HttpResponse::Ok().body("Data pendaki dihapus.")
            }
        }
        Err(e) => HttpResponse::InternalServerError().body(format!("Error: {}", e)),
    }
}

// PUT /api/pendaki/{id} — Edit data pendaki
//
// Task 3.4 — sama seperti `selesaikan_pendakian` dan `hapus_pendaki`:
// 0 row → 404 Not Found, >=1 → 200 OK, error DB → 500.
async fn edit_pendaki(path: web::Path<i32>, data: web::Json<RegistrasiPendaki>, pool: web::Data<Pool<Postgres>>) -> impl Responder {
    let id = path.into_inner();
    let query = "UPDATE pendaki SET nama_pendaki=$1, id_perangkat=$2, telepon_darurat=$3 WHERE id=$4";
    let result = sqlx::query(query)
        .bind(&data.nama_pendaki)
        .bind(&data.id_perangkat)
        .bind(&data.telepon_darurat)
        .bind(id)
        .execute(pool.get_ref())
        .await;
    match result {
        Ok(res) => {
            if res.rows_affected() == 0 {
                HttpResponse::NotFound().body("Pendaki tidak ditemukan.")
            } else {
                HttpResponse::Ok().body("Data pendaki diperbarui.")
            }
        }
        Err(e) => HttpResponse::InternalServerError().body(format!("Error: {}", e)),
    }
}

// GET /api/pendaki/cari?q=... — Cari pendaki berdasarkan nama
async fn cari_pendaki(
    query: web::Query<SearchQuery>,
    pool: web::Data<Pool<Postgres>>,
) -> impl Responder {
    let q = format!("%{}%", query.q);
    let query_str = "SELECT * FROM pendaki WHERE nama_pendaki ILIKE $1 ORDER BY tanggal_naik DESC LIMIT 50";
    
    let records = sqlx::query_as::<_, Pendaki>(query_str)
        .bind(q)
        .fetch_all(pool.get_ref())
        .await;

    match records {
        Ok(data) => HttpResponse::Ok().json(data),
        Err(e) => HttpResponse::InternalServerError().body(format!("Error: {}", e)),
    }
}

// Model untuk menerima permintaan "Kirim Peringatan" dari Web
#[derive(Deserialize)]
struct AlertRequest {
    id_perangkat: String,
    jenis_peringatan: String,
}

// ============================================================================
// SerialCmd (Task 3.2 — Bug B5)
// ----------------------------------------------------------------------------
// Payload JSON outbound ke Heltec Basecamp WAJIB dibangun lewat `serde_json`,
// bukan `format!`, agar karakter spesial (`"`, `\`, `\n`, `}`) di
// `id_perangkat` / `jenis_peringatan` ter-escape sesuai spek JSON dan tidak
// membuka celah JSON injection (mis. `id = "X\",\"cmd\":\"WIPE\""` yang bisa
// menyisipkan perintah kedua).
//
// Field `cmd` sengaja `&'static str` karena nilainya tetap `"VIBRATE"`;
// `target` & `reason` borrow dari `AlertRequest` agar tidak ada alokasi
// ekstra untuk string yang sudah dimiliki handler.
// ============================================================================
#[derive(Serialize)]
struct SerialCmd<'a> {
    target: &'a str,
    cmd: &'static str,
    reason: &'a str,
}

// Endpoint POST: Meneruskan perintah ke Kabel USB (Serial)
//
// Task 3.1 — handler TIDAK lagi memanggil `serialport::open()` (yang dulu
// rebutan port dengan reader → 202 "Simulasi"). Sekarang handler hanya
// mengirim `SerialCommand` ke writer task lewat `mpsc` channel + menunggu
// `oneshot` ack. Writer task adalah satu-satunya pemilik akses tulis ke
// port (lewat `Arc<Mutex<Option<Box<dyn SerialPort + Send>>>>` yang juga
// dipegang reader).
//
// Task 3.2 — payload JSON sekarang dibangun lewat `serde_json::to_vec(
// &SerialCmd { target, cmd: "VIBRATE", reason })`. Ini menutup celah JSON
// injection di B5: nilai `id_perangkat`/`jenis_peringatan` yang berisi
// `"`, `\`, `\n`, atau `}` ter-escape jadi string literal, bukan
// dipotong/disambung mentah ke template `format!`.
async fn kirim_peringatan(
    req: web::Json<AlertRequest>,
    hub: web::Data<SerialHub>,
) -> impl Responder {
    // 1. Bangun payload JSON via struct + serde. Setiap karakter spesial
    //    pada `target`/`reason` ter-escape sesuai spek JSON. `cmd` adalah
    //    konstanta literal `"VIBRATE"`.
    let cmd_struct = SerialCmd {
        target: &req.id_perangkat,
        cmd: "VIBRATE",
        reason: &req.jenis_peringatan,
    };

    let mut payload = match serde_json::to_vec(&cmd_struct) {
        Ok(v) => v,
        Err(e) => {
            // Praktis tidak akan terjadi untuk struct sederhana ini, tapi
            // tetap ditangani — kita tidak boleh meneruskan payload korup
            // ke alat keselamatan.
            println!("❌ Gagal serialize SerialCmd: {}", e);
            return HttpResponse::InternalServerError()
                .body("Gagal membangun payload peringatan.");
        }
    };
    // Heltec firmware memparsing per baris (`\n`-terminated).
    payload.push(b'\n');

    println!("🚨 MENGIRIM PERINTAH DOWNLINK LORA 🚨");
    println!(
        "Data Serial ke USB: {}",
        String::from_utf8_lossy(&payload).trim_end()
    );

    // 2. Bangun SerialCommand + oneshot ack.
    let (ack_tx, ack_rx) = oneshot::channel::<Result<(), String>>();
    let cmd = SerialCommand { payload, ack: ack_tx };

    // 3. Kirim ke writer task via mpsc. Kalau channel ditutup (writer task
    //    panic / belum spawn / dropped) → 503 Service Unavailable.
    if let Err(e) = hub.tx.send(cmd).await {
        println!("⚠️ Serial writer task offline (channel closed): {:?}", e);
        return HttpResponse::ServiceUnavailable()
            .body("Serial writer offline: alat tidak siap menerima peringatan.");
    }

    // 4. Tunggu ack hasil tulis dari writer task. Tiga hasil:
    //    - Ok(Ok(())):  writer berhasil tulis → 200 OK.
    //    - Ok(Err(e)):  port belum siap / I/O error → 503 dengan pesan.
    //    - Err(_):      ack drop tanpa balasan (writer task panic) → 500.
    match ack_rx.await {
        Ok(Ok(())) => {
            println!("✅ Berhasil dikirim ke Heltec Basecamp via Serial!");
            HttpResponse::Ok()
                .body("Berhasil: Perintah peringatan diteruskan ke perangkat Basecamp!")
        }
        Ok(Err(e)) => {
            println!("⚠️ Writer melaporkan port belum siap: {}", e);
            HttpResponse::ServiceUnavailable()
                .body(format!("Serial belum siap: {}", e))
        }
        Err(_) => {
            println!("❌ Ack hilang (writer task panic).");
            HttpResponse::InternalServerError()
                .body("Internal: ack writer task hilang.")
        }
    }
}

// Endpoint GET: Mengecek status perangkat Basecamp (tersambung ke Serial atau tidak)
async fn cek_status() -> impl Responder {
    let port_name = env::var("SERIAL_PORT").unwrap_or_else(|_| "/dev/ttyUSB0".to_string());
    
    // Cek daftar port yang tersedia di sistem
    let ports = serialport::available_ports().unwrap_or_else(|_| vec![]);
    let is_connected = ports.iter().any(|p| p.port_name == port_name);

    let status = if is_connected { "online" } else { "offline" };
    let response_body = format!("{{\"status\":\"{}\", \"port\":\"{}\"}}", status, port_name);
    
    HttpResponse::Ok().content_type("application/json").body(response_body)
}

// --- WebSocket Actor ---
#[derive(Message, Clone)]
#[rtype(result = "()")]
struct WsMessage(String);

struct MyWs {
    rx: Option<broadcast::Receiver<String>>,
}

impl Actor for MyWs {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let mut rx = self.rx.take().unwrap();
        let addr = ctx.address();
        
        actix_web::rt::spawn(async move {
            while let Ok(msg) = rx.recv().await {
                addr.do_send(WsMessage(msg));
            }
        });
    }
}

impl Handler<WsMessage> for MyWs {
    type Result = ();

    fn handle(&mut self, msg: WsMessage, ctx: &mut Self::Context) {
        ctx.text(msg.0);
    }
}
impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for MyWs {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        if let Ok(ws::Message::Ping(msg)) = msg {
            ctx.pong(&msg);
        }
    }
}

// ============================================================================
// MQTT Client (Task 3.7 — Bug B2, B6, B10)
// ----------------------------------------------------------------------------
// B6 — `EventLoop` di rumqttc tidak boleh dipakai ulang setelah error: koneksi
//      TCP-nya sudah putus, tapi state internal-nya menahan client yang lama.
//      Akibatnya setelah broker recover, `eventloop.poll()` akan terus
//      mengembalikan error yang sama dan backend perlu restart manual untuk
//      benar-benar resubscribe (requirement 1.6).
//
// Fix: outer loop SETIAP iterasi membangun ulang `MqttOptions` →
//      `AsyncClient::new` → `EventLoop` baru, lalu subscribe ulang. Saat poll
//      error, kita BREAK dari inner loop, tidur dengan exponential backoff
//      (1s, 2s, 4s, 8s, 16s, capped 30s), lalu kembali ke top untuk rebuild
//      penuh. Backoff direset ke 1s segera setelah broker terlihat sehat
//      (poll mengembalikan Ok pertama) sehingga blip sesaat tidak naik ke 30s.
//
// B2 — Anonymous MQTT publish memungkinkan host nakal mengisi `log_sensor`
//      dengan posisi palsu. Mosquitto sekarang `allow_anonymous false` +
//      `password_file`, dan backend memanggil `set_credentials` dari env
//      `MQTT_USERNAME` / `MQTT_PASSWORD` jika keduanya ada. Saat env tidak
//      di-set (mis. lingkungan dev lokal yang sengaja anon), kita lewati
//      panggilan tersebut sehingga koneksi tetap mungkin ke broker open.
//
// B10 — `QoS::AtMostOnce` tidak memberi jaminan delivery; saat backend restart
//       di tengah pesan in-flight, posisi pendaki bisa hilang. Naik ke
//       `QoS::AtLeastOnce` mengaktifkan retransmit broker → klien, tetapi itu
//       berarti pesan yang sama bisa muncul dua kali. Idempotency dijaga oleh
//       UNIQUE INDEX `log_sensor_dedupe_idx (id_perangkat, timestamp)` di
//       migrasi `main()` plus `INSERT ... ON CONFLICT DO NOTHING` di sini.
async fn start_mqtt_client(
    pool: Pool<Postgres>,
    tx: broadcast::Sender<String>,
    hub: Arc<Mutex<AlertHub>>,
) {
    let host = env::var("MQTT_BROKER_HOST").unwrap_or_else(|_| "localhost".to_string());
    let port = env::var("MQTT_BROKER_PORT")
        .unwrap_or_else(|_| "1883".to_string())
        .parse::<u16>()
        .unwrap_or(1883);

    // Kredensial diambil sekali di awal — env vars dibaca dari `.env` yang
    // sudah di-load di `main()` (dotenv). `Option<String>` agar lingkungan
    // tanpa auth (dev/local) tetap bisa konek ke broker yang masih anon.
    let user = env::var("MQTT_USERNAME").ok();
    let pass = env::var("MQTT_PASSWORD").ok();

    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(30);

    loop {
        // 1. Build sesi baru tiap iterasi — ini wajib untuk B6: client &
        //    eventloop lama tidak recoverable setelah error TCP/proto.
        let mut opts = MqttOptions::new("altivex_backend_cloud", &host, port);
        opts.set_keep_alive(Duration::from_secs(5));
        if let (Some(u), Some(p)) = (user.as_ref(), pass.as_ref()) {
            opts.set_credentials(u, p);
        }
        let (client, mut eventloop) = AsyncClient::new(opts, 10);

        // 2. Subscribe dengan QoS=AtLeastOnce (B10). `client.subscribe()`
        //    di rumqttc cuma queue request ke local channel — confirmation
        //    SUBACK datang lewat eventloop.poll() nanti. Jangan pernah
        //    print "Subscriber aktif" di sini, karena belum terjamin
        //    broker terima auth + subscribe-nya.
        if let Err(e) = client
            .subscribe("altivex/sensor/data", QoS::AtLeastOnce)
            .await
        {
            println!(
                "❌ Gagal queue subscribe MQTT: {:?}. Reconnect dalam {:?}...",
                e, backoff
            );
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(max_backoff);
            continue;
        }

        // Sekalian subscribe ke topic acknowledge dari basecamp ESP32.
        // Saat penjaga tekan tombol fisik, basecamp publish ke
        // `altivex/basecamp/ack` dengan payload `{"id_perangkat": "..."}`
        // atau `{"all": true}` — backend reset notification flag tapi
        // alert tetap "active" di hub sampai kondisi clear.
        if let Err(e) = client
            .subscribe("altivex/basecamp/ack", QoS::AtLeastOnce)
            .await
        {
            println!(
                "⚠️  Gagal queue subscribe basecamp/ack: {:?} (alert ack tidak akan bekerja)",
                e
            );
        }

        // Daftarkan client ini ke AlertHub supaya alert publisher bisa
        // dipakai dari berbagai handler. Re-set tiap iterasi outer loop
        // (pas reconnect) supaya stale client tidak dipakai.
        {
            let mut h = hub.lock().unwrap();
            h.mqtt = Some(client.clone());
        }

        // 3. Inner loop — poll sampai error. `healthy` flag agar kita bisa
        //    drop client + eventloop SETELAH inner loop selesai (lewat
        //    fall-through ke akhir outer loop), bukan dari tengah match.
        let mut healthy = true;
        let mut got_first_ok = false;
        while healthy {
            match eventloop.poll().await {
                Ok(Event::Incoming(Packet::Publish(publish))) => {
                    // Reset backoff begitu broker terbukti sehat — koneksi
                    // yang baru saja blip tidak boleh menyeret backoff ke 30s.
                    if !got_first_ok {
                        backoff = Duration::from_secs(1);
                        got_first_ok = true;
                    }

                    let payload = publish.payload;
                    let topic = publish.topic.as_str();

                    // Routing: dispatch berdasarkan topic. Sensor data
                    // dari pendaki masuk via altivex/sensor/data; ack
                    // dari basecamp ESP32 masuk via altivex/basecamp/ack.
                    if topic == "altivex/basecamp/ack" {
                        // Future hook: kalau penjaga tekan tombol, basecamp
                        // publish payload kecil ke topic ini supaya backend
                        // bisa log "ack diterima". Saat ini basecamp ESP32
                        // sudah handle silence-buzzer secara lokal (tidak
                        // butuh round-trip), tapi log ini berguna untuk
                        // observability — penjaga sebenernya pernah ack atau
                        // ngga.
                        let sample: String = String::from_utf8_lossy(&payload)
                            .chars().take(120).collect();
                        println!("🔕 Basecamp ACK diterima: {}", sample);
                        continue;
                    }

                    if let Ok(data) = serde_json::from_slice::<IncomingData>(&payload) {
                        // Visibility log — sengaja ringkas (tanpa payload
                        // utuh) supaya tidak men-spam log saat publish
                        // tinggi. Operator butuh kepastian "publish masuk"
                        // ke backend sebelum ngecek DB.
                        println!(
                            "📥 MQTT publish diterima: id={} lat={} lon={}",
                            data.id_perangkat, data.latitude, data.longitude
                        );

                        // Task 3.3 — guard koordinat tetap berlaku.
                        if !valid_coord(&data) {
                            println!(
                                "⚠️  Payload sensor ditolak (MQTT altivex/sensor/data) — id='{}', lat={}, lon={}",
                                data.id_perangkat, data.latitude, data.longitude
                            );
                            continue;
                        }

                        // Idempotent insert (B10) — broker bisa retransmit
                        // pesan yang sama, kita absorb via UNIQUE INDEX
                        // `log_sensor_dedupe_idx (id_perangkat, timestamp)`.
                        let battery = sanitize_battery(data.battery);
                        let insert_res = sqlx::query(
                            "INSERT INTO log_sensor (id_perangkat, latitude, longitude, battery) \
                             VALUES ($1, $2, $3, $4) \
                             ON CONFLICT DO NOTHING",
                        )
                        .bind(&data.id_perangkat)
                        .bind(data.latitude)
                        .bind(data.longitude)
                        .bind(battery)
                        .execute(&pool)
                        .await;

                        match insert_res {
                            Ok(r) => {
                                if r.rows_affected() == 0 {
                                    println!(
                                        "↩️  Dedupe (ON CONFLICT DO NOTHING) — id={} sudah ada di window timestamp.",
                                        data.id_perangkat
                                    );
                                } else {
                                    println!(
                                        "💾 Insert OK ke log_sensor: id={} ({} row).",
                                        data.id_perangkat,
                                        r.rows_affected()
                                    );
                                }
                            }
                            Err(e) => {
                                println!(
                                    "❌ Gagal INSERT log_sensor untuk id={}: {}",
                                    data.id_perangkat, e
                                );
                            }
                        }

                        // Broadcast ke WebSocket
                        if let Ok(json_str) = serde_json::to_string(&data) {
                            match tx.send(json_str) {
                                Ok(n) => println!("📣 WS broadcast → {} subscriber.", n),
                                Err(_) => println!("ℹ️  WS belum ada subscriber (skip broadcast)."),
                            }
                        }

                        // Auto-alert evaluation (geofence + battery,
                        // basecamp MQTT push pada transisi state).
                        evaluate_alerts(&hub, &pool, &data).await;
                    } else {
                        // Payload bukan IncomingData JSON valid (mis. ESP32
                        // kirim non-JSON / format salah). Print sample byte
                        // pertama untuk debug — bukan seluruh payload.
                        let sample: String = String::from_utf8_lossy(&payload)
                            .chars()
                            .take(80)
                            .collect();
                        println!(
                            "⚠️  Payload MQTT tidak bisa di-deserialize ke IncomingData. Sample: {:?}",
                            sample
                        );
                    }
                }
                Ok(Event::Incoming(Packet::ConnAck(_))) => {
                    // Auth + handshake sukses. Print SEKARANG, bukan
                    // sebelumnya — pesan ini menjadi indikator nyata
                    // bahwa broker accept credentials kita.
                    println!(
                        "📡 MQTT Subscriber aktif di topic: altivex/sensor/data (QoS=AtLeastOnce)"
                    );
                    if !got_first_ok {
                        backoff = Duration::from_secs(1);
                        got_first_ok = true;
                    }
                }
                Ok(_) => {
                    // SubAck / PingResp — broker hidup. Reset backoff
                    // supaya hiccup berikutnya mulai dari 1s lagi.
                    if !got_first_ok {
                        backoff = Duration::from_secs(1);
                        got_first_ok = true;
                    }
                }
                Err(e) => {
                    println!(
                        "⚠️  MQTT Connection Error: {:?}. Rebuild client + reconnect dalam {:?}...",
                        e, backoff
                    );
                    healthy = false; // jatuh keluar inner loop
                }
            }
        }

        // 4. Tidur dengan backoff lalu rebuild semuanya. `client` & `eventloop`
        //    di-drop di akhir scope outer loop iteration berikutnya — kita
        //    biarkan compiler yang melepasnya.
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(max_backoff);
    }
}

// --- Serial Reader Logic (Local Bridge / Failsafe) ---
//
// Task 3.1 — reader sekarang share port dengan writer task lewat
// `SharedSerialPort = Arc<Mutex<Option<Box<dyn SerialPort + Send>>>>`.
//
// Lifecycle satu siklus:
//   1. Loop coba `serialport::new(...).open()`. Sukses → masukkan port ke
//      `shared_port.lock()` sebagai `Some(port)`, set `connected = true`.
//      Gagal → tidur 5 detik, retry (preserve 3.7).
//   2. Loop baca: setiap iterasi `lock()` port lalu `read()`. Lock dilepas
//      antar baca agar writer task bisa menyelinap menulis perintah.
//   3. Saat error baca (alat dicabut) → set port ke `None`, set `connected
//      = false`, balik ke step 1.
async fn start_serial_reader(
    pool: Pool<Postgres>,
    tx: broadcast::Sender<String>,
    shared_port: SharedSerialPort,
    connected: Arc<AtomicBool>,
) {
    let port_name = env::var("SERIAL_PORT").unwrap_or_else(|_| "COM3".to_string());
    let baud_rate = 115200;

    println!("🔌 Memulai Serial Reader di {}...", port_name);

    loop {
        // Pakai spawn_blocking agar `serialport::open()` (sync I/O) tidak
        // memblokir worker async. Ini juga konsisten dengan rekomendasi
        // requirement 2.4 (no blocking dalam handler async).
        let open_result = {
            let port_name = port_name.clone();
            tokio::task::spawn_blocking(move || {
                serialport::new(&port_name, baud_rate)
                    .timeout(Duration::from_millis(1000))
                    .open()
            })
            .await
        };

        match open_result {
            Ok(Ok(port)) => {
                println!("✅ Terhubung ke Heltec Basecamp via Serial di {}", port_name);
                {
                    let mut guard = shared_port.lock().unwrap();
                    *guard = Some(port);
                }
                connected.store(true, Ordering::SeqCst);

                let mut serial_buf: Vec<u8> = vec![0; 1000];
                let mut line_buf = String::new();

                'read_loop: loop {
                    // Lock port hanya saat benar-benar membaca, lalu
                    // segera lepas — beri kesempatan writer task untuk
                    // mengirim perintah peringatan tanpa starvation.
                    let read_result = {
                        let mut guard = shared_port.lock().unwrap();
                        match guard.as_mut() {
                            Some(port) => port.read(serial_buf.as_mut_slice()),
                            None => {
                                // Port di-take oleh disconnect handler —
                                // keluar untuk reconnect.
                                break 'read_loop;
                            }
                        }
                    };

                    match read_result {
                        Ok(t) => {
                            let s = String::from_utf8_lossy(&serial_buf[..t]);
                            for c in s.chars() {
                                if c == '\n' {
                                    if let Ok(data) =
                                        serde_json::from_str::<IncomingData>(&line_buf)
                                    {
                                        // Task 3.3 — defense-in-depth:
                                        // NEO-6M kadang lock loss → (0,0).
                                        // Skip + log, jangan kontaminasi DB.
                                        if !valid_coord(&data) {
                                            println!(
                                                "⚠️  Payload Serial ditolak — id='{}', lat={}, lon={}",
                                                data.id_perangkat, data.latitude, data.longitude
                                            );
                                            line_buf.clear();
                                            continue;
                                        }

                                        let battery = sanitize_battery(data.battery);
                                        let _ = sqlx::query(
                                            "INSERT INTO log_sensor (id_perangkat, latitude, longitude, battery) VALUES ($1, $2, $3, $4)",
                                        )
                                        .bind(&data.id_perangkat)
                                        .bind(data.latitude)
                                        .bind(data.longitude)
                                        .bind(battery)
                                        .execute(&pool)
                                        .await;

                                        if let Ok(json_str) = serde_json::to_string(&data) {
                                            let _ = tx.send(json_str);
                                        }

                                        println!("📡 Data Serial diterima: {}", line_buf);
                                    }
                                    line_buf.clear();
                                } else if c != '\r' {
                                    line_buf.push(c);
                                }
                            }
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                            // Beri waktu writer untuk mendapatkan lock; tanpa
                            // yield, task ini akan langsung re-lock.
                            tokio::task::yield_now().await;
                        }
                        Err(e) => {
                            println!("❌ Error baca Serial: {:?}. Mencoba reconnect...", e);
                            // Lepas port supaya writer task tahu offline.
                            let mut guard = shared_port.lock().unwrap();
                            *guard = None;
                            connected.store(false, Ordering::SeqCst);
                            break 'read_loop;
                        }
                    }
                }
            }
            Ok(Err(_)) | Err(_) => {
                // Gagal buka port (alat tidak dicolok / spawn_blocking
                // panic / dsb.). Pastikan slot kosong + tidur 5s sebelum
                // retry — preserve perilaku 3.7.
                {
                    let mut guard = shared_port.lock().unwrap();
                    *guard = None;
                }
                connected.store(false, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

/// Writer task — penerima `SerialCommand` dari handler HTTP.
///
/// Jalankan SEKALI di startup. Dia memegang `mpsc::Receiver` (single
/// consumer) sehingga semua handler `kirim_peringatan` paralel diserialkan
/// melalui satu jalur tulis. Saat port `None` (alat belum terhubung) writer
/// membalas `Err("port belum siap")` ke ack channel — handler menerjemahkan
/// itu jadi 503 Service Unavailable (preserve 3.7 sinyal "alat absent").
async fn start_serial_writer(
    mut rx: mpsc::Receiver<SerialCommand>,
    shared_port: SharedSerialPort,
) {
    println!("🛠️  Serial Writer task aktif (mpsc).");
    while let Some(cmd) = rx.recv().await {
        let SerialCommand { payload, ack } = cmd;

        // Operasi tulis adalah sync I/O, bungkus dengan spawn_blocking agar
        // tidak memblokir worker async. `Arc<Mutex<...>>` di-clone ke task
        // blocking, lalu kita ambil lock di dalamnya.
        let port_arc = shared_port.clone();
        let result = tokio::task::spawn_blocking(move || -> Result<(), String> {
            let mut guard = port_arc.lock().map_err(|_| "Mutex poisoned".to_string())?;
            match guard.as_mut() {
                Some(port) => port
                    .write_all(&payload)
                    .and_then(|_| port.flush())
                    .map_err(|e| format!("I/O error: {}", e)),
                None => Err("port belum siap (alat tidak terhubung)".to_string()),
            }
        })
        .await
        .unwrap_or_else(|join_err| Err(format!("writer panic: {}", join_err)));

        // Abaikan SendError jika handler sudah cancel (request drop).
        let _ = ack.send(result);
    }
    println!("⚠️  Serial Writer task berhenti — channel ditutup.");
}

// -----------------------

// Endpoint GET: Menerima koneksi WebSocket
async fn ws_index(
    req: HttpRequest,
    stream: Payload,
    tx: web::Data<broadcast::Sender<String>>,
) -> Result<HttpResponse, Error> {
    let rx = tx.subscribe();
    ws::start(MyWs { rx: Some(rx) }, &req, stream)
}
// -----------------------

// ============================================================================
// AuthMiddleware (Task 3.8 — Bug B3)
// ----------------------------------------------------------------------------
// Middleware Actix berbasis `actix_web::middleware::from_fn` (shipped sejak
// actix-web 4.13). Memverifikasi header `Authorization: Bearer <token>`
// terhadap env `API_AUTH_TOKEN` yang dibaca SEKALI di startup dan disuntikkan
// sebagai `web::Data<AuthConfig>` (tidak baca env per request).
//
// Whitelist path public — match dengan prefix-path agar `Files::new("/")`
// tetap melayani static asset:
//   - "/"               → static frontend (`index.html`).
//   - "/index.html"     → eksplisit untuk request langsung.
//   - "/GEO.json"       → data jalur publik.
//   - "/api/status"     → health check (dipakai dashboard sebelum login).
//   - "/ws"             → WebSocket; browser tidak bisa attach Bearer header
//                          ke handshake WS, jadi route ini publik dengan
//                          alasan teknis. Listener (broadcast) hanya
//                          memancarkan data sensor (no PII), risiko-nya
//                          terbatas.
//
// Path lain (mutating REST + read-only protected seperti `GET /api/sensor`,
// `GET /api/pendaki/*`, `GET /api/history/*`) WAJIB Bearer valid.
//
// Misconfig guard: kalau `API_AUTH_TOKEN` kosong/tidak diset, middleware
// gagal-loud `503 Service Unavailable` agar deploy tidak diam-diam membuka
// pintu. Token TIDAK pernah di-echo balik di body respons.
// ============================================================================

/// Resource auth yang di-share via `web::Data`. Token dibaca SEKALI di
/// startup; bila ingin rotasi, restart proses sesuai pola env-driven
/// config existing.
///
/// Login flow (UI #4 user feedback):
/// - `username` + `password` di-set lewat env `BASECAMP_USERNAME` /
///   `BASECAMP_PASSWORD`. Endpoint `POST /api/login` membandingkan body
///   request dengan dua nilai ini secara constant-time (`subtle`-style
///   manual byte-wise eq). Sukses → return `{ token: API_AUTH_TOKEN }`
///   yang langsung disimpan frontend ke localStorage.
/// - Backend AuthMiddleware tetap memvalidasi `Authorization: Bearer
///   <token>` seperti sebelumnya — login flow hanya selapis UX di atas
///   token mechanism existing, tidak rombak skema otorisasi.
#[derive(Clone)]
struct AuthConfig {
    /// Token yang diharapkan pada header `Authorization: Bearer <token>`.
    /// `String::new()` saat env tidak diset; middleware akan menolak semua
    /// request non-public dengan 503 sampai operator memperbaiki config.
    token: String,
    /// Username basecamp single-user. Kosong saat env tidak diset →
    /// `/api/login` tetap aktif tapi ALWAYS reject (tidak boleh fallback
    /// ke "siapa saja boleh login" — fail-closed).
    username: String,
    /// Password plaintext (env-driven). Untuk versi single-user kita
    /// belum perlu password hash di DB — operator basecamp = 1 orang,
    /// kredensial hanya berpindah lewat secret env yang sudah di-protect.
    /// TODO multi-user: ganti ke argon2 hash di tabel `users`.
    password: String,
}

#[derive(Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Serialize)]
struct LoginResponse {
    token: String,
}

/// `POST /api/login` — UX gate untuk operator basecamp.
///
/// Constant-time compare via byte-wise iteration (sederhana dibanding
/// `subtle::ConstantTimeEq` tapi cukup karena kita selalu compare dua
/// string dengan panjang yang sama setelah panjang-checked di awal).
async fn login(
    body: web::Json<LoginRequest>,
    auth: web::Data<AuthConfig>,
) -> impl Responder {
    // Misconfig: backend belum punya kredensial yang bisa dicocokkan
    // → fail-loud 503 (sama pattern dengan token kosong).
    if auth.username.is_empty() || auth.password.is_empty() || auth.token.is_empty() {
        return HttpResponse::ServiceUnavailable()
            .body("Login belum dikonfigurasi: BASECAMP_USERNAME / BASECAMP_PASSWORD / API_AUTH_TOKEN harus di-set di .env");
    }

    // Constant-time compare. Kita tetap loop sampai akhir even when
    // mismatch ditemukan supaya waktu eksekusi tidak bocor "berapa
    // karakter awal yang match" (timing attack).
    fn ct_eq(a: &str, b: &str) -> bool {
        if a.len() != b.len() {
            return false;
        }
        let mut diff: u8 = 0;
        for (x, y) in a.bytes().zip(b.bytes()) {
            diff |= x ^ y;
        }
        diff == 0
    }

    let user_ok = ct_eq(&body.username, &auth.username);
    let pass_ok = ct_eq(&body.password, &auth.password);

    if !(user_ok && pass_ok) {
        // Body singkat — tidak boleh leak informasi "username valid tapi
        // password salah" karena itu enumerasi user.
        return HttpResponse::Unauthorized()
            .body("Username atau password salah.");
    }

    // Sukses — kembalikan token API. Frontend simpan ke localStorage.
    HttpResponse::Ok().json(LoginResponse {
        token: auth.token.clone(),
    })
}

/// Daftar path publik. Pakai prefix-match (mis. `/` cocok untuk segala
/// static asset di `Files::new("/", "./frontend")` — termasuk
/// `/index.html`, `/GEO.json`, dan asset lain yang akan ditambah operator).
///
/// Kita tetap men-list path publik secara EKSPLISIT (bukan sekadar `/`)
/// agar tabel routing tetap eksplisit & mudah di-audit. Prefix `/`
/// disengaja diletakkan paling akhir karena `starts_with` apa pun cocok;
/// untuk `/api/...` kita ingin gating, bukan auto-allow.
fn is_public_path(path: &str) -> bool {
    // Endpoint API publik & WS — match exact (tidak boleh kebobolan
    // path yang kebetulan diawali `/api/status`).
    if path == "/api/status" || path == "/ws" || path == "/api/login" {
        return true;
    }
    // Static asset publik. `Files` melayani `/`, `/index.html`,
    // `/GEO.json`, dan asset lain (CSS/JS/images) jika nanti ditambah.
    // Asumsi: tidak ada static file di `frontend/` yang sensitif.
    if path == "/" || path == "/index.html" || path == "/GEO.json" {
        return true;
    }
    // Setiap request yang BUKAN `/api/...` dan BUKAN `/ws` kita anggap
    // static asset (dilayani `Files`). Daripada me-list semua asset
    // satu per satu, kita whitelist: "non-API path" → publik.
    !path.starts_with("/api/") && path != "/ws"
}

/// Middleware function untuk `from_fn`. Mengembalikan `ServiceResponse<B>`
/// supaya body type tidak perlu di-bridge dengan `EitherBody`. Untuk respons
/// auth-failure, kita short-circuit lewat `req.into_response(...)` lalu
/// `.map_into_boxed_body()` agar body type-nya selaras dengan inner service.
async fn auth_middleware(
    auth: web::Data<AuthConfig>,
    req: ServiceRequest,
    next: Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<actix_web::body::BoxBody>, Error> {
    // 1. Bypass path publik (static + /api/status + /ws).
    let path = req.path();
    if is_public_path(path) {
        return next.call(req).await.map(|res| res.map_into_boxed_body());
    }

    // 2. Misconfig: token kosong → 503 Service Unavailable. Fail-loud.
    if auth.token.is_empty() {
        let resp = HttpResponse::ServiceUnavailable()
            .body("API_AUTH_TOKEN belum dikonfigurasi di server.");
        return Ok(req.into_response(resp));
    }

    // 3. Ekstrak header `Authorization`. Hindari leak token di error body.
    let header = req
        .headers()
        .get(actix_web::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // Bandingkan header `Bearer <token>`. Strip prefix lebih dulu lalu
    // bandingkan suffix-nya — menghindari format string yang membaca token
    // di tempat yang tidak perlu.
    let provided = header.strip_prefix("Bearer ").unwrap_or("");
    if !provided.is_empty() && provided == auth.token {
        return next.call(req).await.map(|res| res.map_into_boxed_body());
    }

    // 4. Tolak. Body singkat tanpa membocorkan token.
    let resp = HttpResponse::Unauthorized().body("Unauthorized");
    Ok(req.into_response(resp))
}
// -----------------------

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL tidak ditemukan");

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Gagal koneksi ke PostgreSQL");

    // Inisialisasi Broadcast Channel untuk WebSockets
    let (tx, _) = broadcast::channel(100);
    let tx_data = web::Data::new(tx.clone());

    // INIT: Membuat tabel secara otomatis jika belum ada di database
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS log_sensor (
            id SERIAL PRIMARY KEY,
            id_perangkat VARCHAR(50) NOT NULL,
            latitude DOUBLE PRECISION NOT NULL,
            longitude DOUBLE PRECISION NOT NULL,
            timestamp TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        );"
    )
    .execute(&pool)
    .await
    .expect("Gagal membuat tabel log_sensor");

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS pendaki (
            id SERIAL PRIMARY KEY,
            nama_pendaki VARCHAR(255) NOT NULL,
            id_perangkat VARCHAR(50) NOT NULL,
            telepon_darurat VARCHAR(20) NOT NULL DEFAULT '',
            tanggal_naik TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            status VARCHAR(50) NOT NULL
        );"
    )
    .execute(&pool)
    .await
    .expect("Gagal membuat tabel pendaki");

    // Task 3.5 (Bug B9) — kolom `tanggal_turun` untuk window pendakian.
    // Idempotent (`ADD COLUMN IF NOT EXISTS`) sehingga aman dijalankan
    // di DB lama yang sudah berisi data (preserve clause 3.5).
    sqlx::query(
        "ALTER TABLE pendaki ADD COLUMN IF NOT EXISTS tanggal_turun TIMESTAMP NULL;"
    )
    .execute(&pool)
    .await
    .expect("Gagal menambah kolom tanggal_turun");

    // Battery monitor (post-feedback) — kolom persen baterai 0-100
    // dari transmitter pendaki. NULL untuk pesan dari firmware lama
    // yang belum kirim battery. Idempotent ALTER.
    sqlx::query(
        "ALTER TABLE log_sensor ADD COLUMN IF NOT EXISTS battery SMALLINT NULL;"
    )
    .execute(&pool)
    .await
    .expect("Gagal menambah kolom battery di log_sensor");

    // MIGRASI: Tambahkan kolom telepon_darurat jika tabel lama belum punya
    let _ = sqlx::query("ALTER TABLE pendaki ADD COLUMN IF NOT EXISTS telepon_darurat VARCHAR(20) NOT NULL DEFAULT '';")
        .execute(&pool)
        .await;

    // Task 3.7 (Bug B10) — UNIQUE INDEX untuk dedupe insert MQTT yang
    // mungkin di-retransmit broker setelah backend naik QoS ke
    // `AtLeastOnce`. Kombinasi `(id_perangkat, timestamp)` cukup karena
    // perangkat tidak akan kirim dua reading di nanosecond yang sama;
    // INSERT lewat MQTT branch sekarang `ON CONFLICT DO NOTHING`.
    // Idempotent — preserve clause 3.5 (DDL aman dijalankan ulang).
    sqlx::query(
        "CREATE UNIQUE INDEX IF NOT EXISTS log_sensor_dedupe_idx \
         ON log_sensor (id_perangkat, timestamp);"
    )
    .execute(&pool)
    .await
    .expect("Gagal membuat index log_sensor_dedupe_idx");

    println!("✅ Database siap. Tabel log_sensor dan pendaki (dengan kolom telepon_darurat) tersedia.");

    // ------------------------------------------------------------------
    // Auto-Alert Hub (geofence + battery + signal-lost watcher)
    // ------------------------------------------------------------------
    // Load GEO.json sekali di startup. Kalau gagal (file tidak ada,
    // JSON malformed, dst.) → fail-open: alert geofence tidak aktif,
    // tapi alert battery + signal-lost tetap jalan.
    let geofence_path = env::var("GEOFENCE_PATH")
        .unwrap_or_else(|_| "./frontend/GEO.json".to_string());
    let geofence = load_geofence(&geofence_path);
    match &geofence {
        Some(mp) => println!(
            "✅ Geofence ke-load dari '{}' ({} polygon segments).",
            geofence_path,
            mp.0.len()
        ),
        None => println!(
            "⚠️  GEO.json tidak ke-load dari '{}' — alert OUT_OF_GEOFENCE dinonaktifkan.",
            geofence_path
        ),
    }
    let alert_hub = Arc::new(Mutex::new(AlertHub::new(geofence)));

    // Spawn signal-lost watcher (periodic 30 detik).
    let hub_watcher = alert_hub.clone();
    let pool_watcher = pool.clone();
    tokio::spawn(async move {
        start_signal_lost_watcher(hub_watcher, pool_watcher).await;
    });

    // Jalankan MQTT Client di background
    let pool_mqtt = pool.clone();
    let tx_mqtt = tx.clone();
    let hub_mqtt = alert_hub.clone();
    tokio::spawn(async move {
        start_mqtt_client(pool_mqtt, tx_mqtt, hub_mqtt).await;
    });

    // ------------------------------------------------------------------
    // SerialHub (Task 3.1 — B1, B4)
    // ------------------------------------------------------------------
    // Kita buat satu instance shared port + satu mpsc channel. Reader task
    // me-manage open/reconnect; writer task me-listen mpsc dan menulis ke
    // port yang sama via shared mutex. Handler `kirim_peringatan` cukup
    // pegang `web::Data<SerialHub>`.
    let shared_port: SharedSerialPort = Arc::new(Mutex::new(None));
    let connected = Arc::new(AtomicBool::new(false));
    let (serial_tx, serial_rx) = mpsc::channel::<SerialCommand>(32);
    let serial_hub = SerialHub {
        tx: serial_tx,
        connected: connected.clone(),
    };
    let serial_hub_data = web::Data::new(serial_hub);

    // Spawn reader (manages port lifecycle + retry 5s saat absent).
    let pool_serial = pool.clone();
    let tx_serial = tx.clone();
    let shared_port_reader = shared_port.clone();
    let connected_reader = connected.clone();
    tokio::spawn(async move {
        start_serial_reader(pool_serial, tx_serial, shared_port_reader, connected_reader).await;
    });

    // Spawn writer task (single consumer mpsc → tulis ke port yang sama).
    let shared_port_writer = shared_port.clone();
    tokio::spawn(async move {
        start_serial_writer(serial_rx, shared_port_writer).await;
    });

    println!("🚀 Server ALTIVEX berjalan di http://0.0.0.0:8080");

    // Task 3.8 (Bug B3) — baca API_AUTH_TOKEN sekali di startup. Token
    // disimpan di `web::Data<AuthConfig>` agar middleware tidak perlu
    // memanggil `env::var()` per request. Empty/missing token akan
    // membuat middleware menolak semua request non-public dengan 503
    // (fail-loud) — operator HARUS set env sebelum deploy.
    let api_auth_token = env::var("API_AUTH_TOKEN").unwrap_or_default();
    let basecamp_username = env::var("BASECAMP_USERNAME").unwrap_or_default();
    let basecamp_password = env::var("BASECAMP_PASSWORD").unwrap_or_default();
    if api_auth_token.is_empty() {
        println!(
            "⚠️  API_AUTH_TOKEN belum diset — endpoint mutating akan menolak \
             dengan 503 sampai env diisi."
        );
    } else {
        println!("🔐 AuthMiddleware aktif untuk endpoint mutating.");
    }
    if basecamp_username.is_empty() || basecamp_password.is_empty() {
        println!(
            "⚠️  BASECAMP_USERNAME / BASECAMP_PASSWORD belum diset — \
             /api/login akan menolak semua request sampai env diisi."
        );
    } else {
        println!("🔑 Login basecamp aktif untuk user: {}", basecamp_username);
    }
    let auth_config_data = web::Data::new(AuthConfig {
        token: api_auth_token,
        username: basecamp_username,
        password: basecamp_password,
    });

    // Mendaftarkan Endpoint (Routing URL)
    HttpServer::new(move || {
        App::new()
            // Auth middleware aktif untuk seluruh App. Whitelist path
            // public (static, /api/status, /ws) di-handle di dalam
            // middleware via `is_public_path`.
            .wrap(from_fn(auth_middleware))
            .app_data(web::Data::new(pool.clone()))
            .app_data(tx_data.clone())
            .app_data(serial_hub_data.clone())
            .app_data(web::Data::new(alert_hub.clone()))
            .app_data(auth_config_data.clone())
            .route("/api/sensor", web::post().to(terima_data))
            .route("/api/sensor", web::get().to(ambil_data))
            .route("/api/sensor/latest", web::get().to(ambil_sensor_latest))
            .route("/api/history/{id}", web::get().to(ambil_history))
            .route("/api/pendaki/riwayat", web::get().to(ambil_riwayat_pendaki))
            .route("/api/pendaki/cari", web::get().to(cari_pendaki))
            .route("/api/pendaki/{id}/history", web::get().to(ambil_history_pendaki))
            .route("/api/pendaki", web::get().to(ambil_pendaki))
            .route("/api/pendaki", web::post().to(registrasi_pendaki))
            .route("/api/pendaki/{id}", web::put().to(edit_pendaki))
            .route("/api/pendaki/{id}", web::delete().to(hapus_pendaki))
            .route("/api/pendaki/{id}/selesai", web::put().to(selesaikan_pendakian))
            .route("/api/alert", web::post().to(kirim_peringatan))
            .route("/api/status", web::get().to(cek_status))
            .route("/api/login", web::post().to(login))
            .route("/ws", web::get().to(ws_index))
            .service(Files::new("/", "./frontend").index_file("index.html"))
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}
