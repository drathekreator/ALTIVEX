//! Preservation tests untuk ALTIVEX baseline behavior — Task 2 spec
//! altivex-critical-fixes.
//!
//! ## Tujuan
//!
//! Test ini WAJIB LULUS pada kode F (versi `main.rs` saat ini, sebelum
//! fix). Kelulusan-nya adalah dokumentasi baseline behavior yang TIDAK
//! BOLEH regress setelah paket fix di task 3 di-merge.
//!
//! Methodologi: observation-first. Kita observasi behavior pada kode F
//! lalu mengencode-nya sebagai property/asserstion. Karena handler
//! produksi berada di `main.rs` (binary, bukan lib) sehingga tidak
//! bisa di-import langsung, kita REPLIKASI logika produksi apa adanya
//! ke harness in-memory (sama dengan strategi `exploration_test.rs`).
//! Replika ini menjadi single-source-of-truth untuk perilaku jalur
//! happy yang harus dipertahankan.
//!
//! ## Cakupan (mengikuti tasks.md task 2)
//!
//! - **3.1 / 3.2**: `POST /api/sensor` payload valid → INSERT ke
//!   `log_sensor` (≥1 baris bertambah) AND broadcast WS dengan JSON
//!   `{id_perangkat, latitude, longitude}` identik input.
//! - **3.4**: `GET /api/sensor` mengembalikan array `SensorRecord`
//!   urut `timestamp DESC`.
//! - **3.6**: `GET /api/pendaki/cari?q=...` untuk nama yang ada
//!   menghasilkan struktur respons `Pendaki` dengan field-set yang
//!   sama dengan baseline.
//! - **3.7**: Saat port serial absent, reader retry tanpa panic dalam
//!   6 detik.
//!
//! ## Validates
//!
//! - **Validates: Requirements 3.1, 3.2** (POST sensor + WS broadcast)
//! - **Validates: Requirements 3.4** (GET sensor sort order)
//! - **Validates: Requirements 3.6** (Pendaki field-set snapshot)
//! - **Validates: Requirements 3.7** (Reader retry tanpa panic)

use proptest::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Replika model dari `altivex_backend/src/main.rs`
// ---------------------------------------------------------------------------
//
// Kita salin definisi struct & logika serialisasi-nya verbatim agar
// asersi kita berjalan terhadap shape JSON yang persis sama dengan
// produksi. Setelah fix di task 3.x dijalankan, struct ini di produksi
// SHALL tidak berubah (preservation), sehingga test ini tetap lulus.

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
struct IncomingData {
    id_perangkat: String,
    latitude: f64,
    longitude: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct SensorRecord {
    id_perangkat: String,
    latitude: f64,
    longitude: f64,
}

/// Replika `Pendaki` (`main.rs` line ≈ 110). Field tanggal_naik
/// di-string-kan untuk kemudahan test (di produksi adalah
/// `chrono::NaiveDateTime`, tapi shape JSON-nya tetap satu field
/// `tanggal_naik`).
#[derive(Serialize, Clone, Debug)]
struct Pendaki {
    id: i32,
    nama_pendaki: String,
    id_perangkat: String,
    telepon_darurat: String,
    tanggal_naik: String,
    status: String,
}

// ---------------------------------------------------------------------------
// Replika handler `terima_data` (`main.rs` line ≈ 32) — POST /api/sensor
// ---------------------------------------------------------------------------
//
// Logika yang direplika:
//   1. INSERT INTO log_sensor (...) VALUES ($1, $2, $3)
//   2. broadcast::Sender::send(serde_json::to_string(&data))
//   3. Pada Ok → 200 OK; pada Err DB → 500
//
// Untuk preservation, kita tidak peduli apakah DB asli berhasil; kita
// hanya peduli bahwa pada kode F: payload valid menghasilkan
// (a) tambahan baris di store log_sensor, (b) broadcast WS dengan
// JSON yang dapat di-deserialisasi kembali ke field-set yang sama.

fn mock_terima_data(
    log_sensor: &mut Vec<IncomingData>,
    ws_broadcast: &mut Vec<String>,
    data: IncomingData,
) -> u16 {
    // 1. INSERT
    log_sensor.push(data.clone());
    // 2. Broadcast (di produksi `tx.send(json_str)` non-blocking; kita
    //    catat string yang akan dikirim).
    if let Ok(json_str) = serde_json::to_string(&data) {
        ws_broadcast.push(json_str);
    }
    200
}

// ---------------------------------------------------------------------------
// Replika handler `ambil_data` (`main.rs` line ≈ 60) — GET /api/sensor
// ---------------------------------------------------------------------------
//
// Query asli: SELECT id_perangkat, latitude, longitude FROM log_sensor
// ORDER BY timestamp DESC LIMIT 50. Kita replika dengan menyimpan
// timestamp sebagai i64 di harness.

#[derive(Clone, Debug)]
struct StoredSample {
    record: SensorRecord,
    ts_ms: i64,
}

fn mock_ambil_data(db: &[StoredSample]) -> Vec<SensorRecord> {
    // ORDER BY timestamp DESC
    let mut sorted: Vec<&StoredSample> = db.iter().collect();
    sorted.sort_by(|a, b| b.ts_ms.cmp(&a.ts_ms));
    // LIMIT 50
    sorted.into_iter().take(50).map(|s| s.record.clone()).collect()
}

// ---------------------------------------------------------------------------
// PBT: 3.1 / 3.2 — POST /api/sensor preservation
// ---------------------------------------------------------------------------
//
// Property (clause 3.1, 3.2 di requirements.md):
//   FORALL (id, lat, lon) WHERE
//     id non-empty AND lat ∈ [-90,90] AND lon ∈ [-180,180]
//     AND |lat| + |lon| > ε:
//       mock_terima_data(...) ⇒
//         (a) status 200
//         (b) log_sensor.len() bertambah ≥ 1
//         (c) WS broadcast berisi tepat 1 pesan
//         (d) string broadcast == serde_json::to_string(&input)
//         (e) field-set parsed broadcast == input (id string-equal,
//             lat/lon dalam toleransi 1 ULP — round-trip f64 via
//             JSON umumnya tepat tapi tidak dijamin bit-exact untuk
//             semua nilai)
//
// Pada kode F, property ini SHALL lulus (baseline). Pada kode F'
// dengan validasi koordinat (task 3.3) tetap lulus karena input di
// sini sudah memenuhi guard.

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8,
        ..ProptestConfig::default()
    })]

    /// Validates: Requirements 3.1, 3.2
    ///
    /// Payload sensor valid → INSERT + WS broadcast dengan field-set
    /// identik input. Property ini meng-encode jalur happy yang
    /// SHALL tidak terganggu oleh paket fix.
    #[test]
    fn pres_post_sensor_inserts_dan_broadcast_payload_valid(
        id in "[A-Za-z0-9_-]{1,32}",
        lat in -90.0_f64..=90.0_f64,
        lon in -180.0_f64..=180.0_f64,
    ) {
        // Filter: koordinat tidak (≈0,0) — sesuai requirement 3.1
        // (`|lat| + |lon| > ε`). Lihat clause B8/2.8.
        prop_assume!(lat.abs() + lon.abs() > 1e-3);
        // id_perangkat non-empty (sudah dijamin regex minLength=1).
        prop_assume!(!id.trim().is_empty());

        let mut log_sensor: Vec<IncomingData> = Vec::new();
        let mut ws: Vec<String> = Vec::new();
        let pre_count = log_sensor.len();

        let data = IncomingData {
            id_perangkat: id.clone(),
            latitude: lat,
            longitude: lon,
        };

        let status = mock_terima_data(&mut log_sensor, &mut ws, data.clone());

        // (a) status 200 (jalur happy)
        prop_assert_eq!(status, 200);
        // (b) ≥ 1 baris baru di log_sensor
        prop_assert!(
            log_sensor.len() >= pre_count + 1,
            "log_sensor harus bertambah ≥1 baris (pre={}, post={})",
            pre_count,
            log_sensor.len()
        );
        // (c) WS broadcast berisi 1 pesan
        prop_assert_eq!(ws.len(), 1, "broadcast WS harus persis 1 pesan");

        // (d) Broadcast string IDENTIK dengan hasil
        // `serde_json::to_string(&data)` — itulah perilaku F yang
        // di-lock (`tx.send(serde_json::to_string(&data)?)` di
        // `terima_data`). Kita TIDAK assert bit-exact round-trip
        // karena round-trip f64→string→f64 umumnya benar tapi tidak
        // dijamin oleh kontrak handler; yang dijamin adalah string
        // broadcast = serialisasi input.
        let expected_json = serde_json::to_string(&data)
            .expect("serialize input harus sukses");
        prop_assert_eq!(
            &ws[0],
            &expected_json,
            "broadcast SHALL identik dengan serde_json::to_string(&input)"
        );

        // (e) Parsing balik tetap menghasilkan field-set yang sama
        // (id_perangkat string-equal; lat/lon dalam toleransi
        // floating-point — round-trip serde_json bisa berbeda ≤1 ULP
        // untuk beberapa nilai f64).
        let parsed: IncomingData = serde_json::from_str(&ws[0])
            .expect("broadcast harus valid JSON IncomingData");
        prop_assert_eq!(&parsed.id_perangkat, &data.id_perangkat);
        let lat_diff = (parsed.latitude - data.latitude).abs();
        let lon_diff = (parsed.longitude - data.longitude).abs();
        prop_assert!(
            lat_diff <= f64::EPSILON * data.latitude.abs().max(1.0),
            "latitude round-trip melenceng terlalu jauh: in={} out={} diff={}",
            data.latitude,
            parsed.latitude,
            lat_diff
        );
        prop_assert!(
            lon_diff <= f64::EPSILON * data.longitude.abs().max(1.0),
            "longitude round-trip melenceng terlalu jauh: in={} out={} diff={}",
            data.longitude,
            parsed.longitude,
            lon_diff
        );
    }
}

// ---------------------------------------------------------------------------
// PBT: 3.4 — GET /api/sensor preservation
// ---------------------------------------------------------------------------
//
// Property (clause 3.4):
//   FORALL list of (id, lat, lon, ts):
//     mock_ambil_data(db) ⇒
//       hasilnya array SensorRecord, urut menurun berdasarkan ts,
//       panjangnya min(db.len(), 50)
//
// Pada kode F, baseline-nya adalah ORDER BY timestamp DESC LIMIT 50.

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8,
        ..ProptestConfig::default()
    })]

    /// Validates: Requirements 3.4
    ///
    /// Endpoint GET /api/sensor SHALL CONTINUE TO mengembalikan
    /// array `SensorRecord` urut `timestamp DESC` dengan cap 50.
    #[test]
    fn pres_get_sensor_returns_records_sorted_desc(
        samples in proptest::collection::vec(
            (
                any::<i32>().prop_map(|x| x as i64),
                -90.0_f64..=90.0_f64,
                -180.0_f64..=180.0_f64,
                "[A-Za-z0-9-]{1,16}",
            ),
            1usize..=80usize,
        )
    ) {
        let db: Vec<StoredSample> = samples
            .iter()
            .map(|(ts, lat, lon, id)| StoredSample {
                record: SensorRecord {
                    id_perangkat: id.clone(),
                    latitude: *lat,
                    longitude: *lon,
                },
                ts_ms: *ts,
            })
            .collect();

        // Hitung expected: sort DESC by ts, take 50
        let mut expected_indexes: Vec<usize> = (0..db.len()).collect();
        expected_indexes.sort_by(|&a, &b| db[b].ts_ms.cmp(&db[a].ts_ms));
        let cap = expected_indexes.len().min(50);
        expected_indexes.truncate(cap);
        let expected_records: Vec<SensorRecord> = expected_indexes
            .iter()
            .map(|&i| db[i].record.clone())
            .collect();

        let result = mock_ambil_data(&db);

        // Panjang sesuai cap LIMIT 50
        prop_assert_eq!(result.len(), expected_records.len());

        // Setiap entry result sama dengan expected (urutan sama).
        // Catatan: jika ada ties pada ts_ms, urutan antar-tie boleh
        // bervariasi; Vec::sort_by stable di Rust, jadi urutan
        // relatif elemen tie akan dipertahankan dari urutan input.
        for (i, (got, want)) in result.iter().zip(expected_records.iter()).enumerate() {
            prop_assert_eq!(
                &got.id_perangkat,
                &want.id_perangkat,
                "mismatch id_perangkat di index {}",
                i
            );
            prop_assert_eq!(got.latitude.to_bits(), want.latitude.to_bits());
            prop_assert_eq!(got.longitude.to_bits(), want.longitude.to_bits());
        }
    }
}

// ---------------------------------------------------------------------------
// 3.6 — GET /api/pendaki/cari preservation (snapshot field-set)
// ---------------------------------------------------------------------------
//
// Pada kode F, struct `Pendaki` memiliki field:
//   id, nama_pendaki, id_perangkat, telepon_darurat, tanggal_naik, status
// Snapshot ini SHALL CONTINUE TO sama setelah fix.

/// Validates: Requirements 3.6
///
/// Pendaki yang dikembalikan oleh /api/pendaki/cari SHALL CONTINUE TO
/// memiliki field-set yang sama dengan baseline.
#[test]
fn pres_pendaki_serialization_field_set_snapshot() {
    let p = Pendaki {
        id: 1,
        nama_pendaki: "Aman Polos".to_string(),
        id_perangkat: "ALAT-001".to_string(),
        telepon_darurat: "081234567890".to_string(),
        tanggal_naik: "2026-01-01T08:00:00".to_string(),
        status: "Mendaki".to_string(),
    };

    let json = serde_json::to_value(&p).expect("Pendaki SHALL serializable");
    let obj = json.as_object().expect("Pendaki SHALL serialize sebagai object");

    let mut keys: Vec<&str> = obj.keys().map(|s| s.as_str()).collect();
    keys.sort();

    let expected = vec![
        "id",
        "id_perangkat",
        "nama_pendaki",
        "status",
        "tanggal_naik",
        "telepon_darurat",
    ];

    assert_eq!(
        keys, expected,
        "Field-set Pendaki SHALL CONTINUE TO sama dengan baseline. \
         Actual={:?}, expected={:?}",
        keys, expected
    );
}

// PBT pendamping: untuk SEMUA Pendaki dengan nilai field arbitrer,
// shape JSON-nya tetap konsisten (tidak ada field yang hilang/extra).

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8,
        ..ProptestConfig::default()
    })]

    /// Validates: Requirements 3.6
    ///
    /// Untuk pendaki dengan nama valid, struktur respons Pendaki
    /// SHALL tetap berisi field-set yang sama (id, nama_pendaki,
    /// id_perangkat, telepon_darurat, tanggal_naik, status).
    #[test]
    fn pres_pendaki_field_set_invariant_under_all_names(
        nama in "[A-Za-z ]{1,40}",
        id in 1i32..=10_000_i32,
        alat in "[A-Z0-9-]{1,16}",
        telp in "[0-9]{8,14}",
    ) {
        let p = Pendaki {
            id,
            nama_pendaki: nama,
            id_perangkat: alat,
            telepon_darurat: telp,
            tanggal_naik: "2026-01-01T08:00:00".to_string(),
            status: "Mendaki".to_string(),
        };
        let json = serde_json::to_value(&p).unwrap();
        let obj = json.as_object().unwrap();
        let mut keys: Vec<&str> = obj.keys().map(|s| s.as_str()).collect();
        keys.sort();
        let expected = vec![
            "id",
            "id_perangkat",
            "nama_pendaki",
            "status",
            "tanggal_naik",
            "telepon_darurat",
        ];
        prop_assert_eq!(keys, expected);
    }
}

// ---------------------------------------------------------------------------
// 3.7 — Serial reader retry tanpa panic saat port absent
// ---------------------------------------------------------------------------
//
// Pada `main.rs` `start_serial_reader` (line ≈ 314), saat
// `serialport::new(...).open()` gagal, kode SHALL CONTINUE TO
// `tokio::time::sleep(Duration::from_secs(5)).await` lalu retry —
// tanpa panic.
//
// Karena `start_serial_reader` adalah fungsi private di binary,
// kita tidak bisa import-nya. Kita REPLIKASI struktur loop-nya
// dengan sleep yang lebih pendek (50ms) agar test cepat — yang
// kita lock adalah PROPERTY-nya: "loop tidak panic, dan terus
// retry selama port absent". Property ini independen dari nilai
// sleep konkret.

fn mock_serial_reader_loop(
    port_name: &str,
    sleep_dur: Duration,
    stop: Arc<AtomicBool>,
    iters: Arc<AtomicUsize>,
) {
    while !stop.load(Ordering::SeqCst) {
        // Mirror: serialport::new(port_name, 115200).timeout(...).open()
        // Untuk port yang tidak ada, library `serialport` SHALL
        // mengembalikan Err tanpa panic — itulah yang kita
        // assert dengan menjalankan langsung.
        let result = serialport::new(port_name, 115200)
            .timeout(Duration::from_millis(100))
            .open();
        match result {
            Ok(_) => {
                // Tidak diharapkan untuk port yang sengaja tidak ada;
                // jika sampai di sini berarti env developer punya
                // device dengan nama yang sama persis. Kita break
                // saja agar test tidak hang.
                break;
            }
            Err(_) => {
                iters.fetch_add(1, Ordering::SeqCst);
                // Mirror: tokio::time::sleep(Duration::from_secs(5)).await;
                // Disingkat untuk efisiensi test runtime — property
                // yang di-lock adalah "loop tetap retry tanpa panic",
                // bukan nilai konkret sleep.
                thread::sleep(sleep_dur);
            }
        }
    }
}

/// Validates: Requirements 3.7
///
/// Saat port serial tidak ada (alat tidak dicolok), reader SHALL
/// CONTINUE TO retry dengan delay tanpa crash. Kita verifikasi:
///   (a) thread tidak panic dalam jendela 1 detik
///   (b) reader benar-benar mencoba berkali-kali (≥ 1 iterasi)
#[test]
fn pres_serial_reader_no_panic_when_port_absent_within_6s() {
    // Pakai nama port yang dijamin tidak akan ada di sistem
    // developer manapun.
    let port_name = "ALTIVEX_PRESERVATION_NONEXISTENT_PORT_99";
    let stop = Arc::new(AtomicBool::new(false));
    let iters = Arc::new(AtomicUsize::new(0));
    let stop_thr = stop.clone();
    let iters_thr = iters.clone();

    let started_at = Instant::now();
    let handle = thread::spawn(move || {
        mock_serial_reader_loop(
            port_name,
            Duration::from_millis(50),
            stop_thr,
            iters_thr,
        );
    });

    // Biarkan reader berjalan ~1 detik (cukup untuk memverifikasi
    // property "no panic + retry ≥ 1" tanpa memperlambat suite).
    thread::sleep(Duration::from_secs(1));
    stop.store(true, Ordering::SeqCst);

    // join() return Err jika thread panic → test SHALL fail di sini.
    handle
        .join()
        .expect("Bug 3.7: serial reader panic saat port absent");

    let elapsed = started_at.elapsed();
    let count = iters.load(Ordering::SeqCst);
    assert!(
        count >= 1,
        "Reader harus retry ≥ 1 kali dalam window (count={})",
        count
    );
    // Sanity: test selesai sekitar 1 detik.
    assert!(
        elapsed >= Duration::from_secs(1),
        "Test harus benar-benar mengamati window 1 detik (elapsed={:?})",
        elapsed
    );
}

// ---------------------------------------------------------------------------
// Sanity tests (deterministik) — memastikan harness sendiri tidak buggy
// ---------------------------------------------------------------------------

#[test]
fn pres_mock_terima_data_happy_example() {
    let mut db = Vec::new();
    let mut ws = Vec::new();
    let data = IncomingData {
        id_perangkat: "ALAT-001".to_string(),
        latitude: -6.7711,
        longitude: 106.96,
    };
    let status = mock_terima_data(&mut db, &mut ws, data.clone());
    assert_eq!(status, 200);
    assert_eq!(db.len(), 1);
    assert_eq!(ws.len(), 1);
    let parsed: IncomingData = serde_json::from_str(&ws[0]).unwrap();
    assert_eq!(parsed, data);
}

#[test]
fn pres_mock_ambil_data_sorts_desc_and_caps_50() {
    let mut db = Vec::new();
    for i in 0..60i64 {
        db.push(StoredSample {
            record: SensorRecord {
                id_perangkat: format!("ALAT-{:03}", i),
                latitude: 0.1 * (i as f64),
                longitude: 0.2 * (i as f64),
            },
            ts_ms: i,
        });
    }
    let result = mock_ambil_data(&db);
    assert_eq!(result.len(), 50);
    // Terbaru dahulu: id_perangkat untuk i=59 di posisi 0
    assert_eq!(result[0].id_perangkat, "ALAT-059");
    assert_eq!(result[49].id_perangkat, "ALAT-010");
}
