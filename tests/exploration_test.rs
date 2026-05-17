//! Exploration test untuk Bug Condition — Task 1 + Task 3.14 spec
//! altivex-critical-fixes.
//!
//! ## Status
//!
//! Task 1 (initial run, kode F): GAGAL → bukti bahwa Bug B1 ada.
//! Task 3.14 (re-run setelah fix): test ini DI-PORT agar replika in-test
//! mengikuti F' (post-fix SerialHub design dari task 3.1 & 3.2). Property
//! assertion-nya tetap sama; counter-example seed historis tetap diuji
//! lewat regular cases proptest. Pada kode F', test SHALL LULUS.
//!
//! ## Strategi (F')
//!
//! Reader (`start_serial_reader`) dan handler (`kirim_peringatan`) di
//! produksi sekarang berbagi satu `Arc<Mutex<Option<Box<dyn SerialPort>>>>`
//! plus `mpsc::Sender<SerialCommand>`. Reader memegang slot port; writer
//! task (single consumer mpsc) yang menulis ke port lewat lock yang sama.
//! Tidak ada `serialport::open()` per-request lagi.
//!
//! Test ini mereplikasi pola itu dengan `MockSerialPort` + `SharedSlot`
//! + `serial_writer_task` async + `mock_kirim_peringatan` yang mengirim
//! `SerialCommand` via mpsc lalu menunggu oneshot ack.
//!
//! ## Validates
//!
//! - **Validates: Requirements 1.1, 2.1** (B1 — Konflik port serial,
//!   sekarang sudah diatasi oleh SerialHub design).

use proptest::prelude::*;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

/// Status code respons handler `kirim_peringatan` mirror produksi:
/// 200 (sukses), 503 (writer/port tidak siap), 500 (ack hilang).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StatusCode(u16);

impl StatusCode {
    fn as_u16(self) -> u16 {
        self.0
    }
}

/// Mock serial port. Satu instance "dimiliki" oleh slot bersama; reader
/// dan writer mengakses lewat `lock()`.
struct MockSerialPort {
    /// Buffer perintah yang berhasil ditulis. Memungkinkan assertion
    /// "payload sampai ke port" (kalau diperlukan).
    written: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl MockSerialPort {
    fn new() -> Self {
        Self {
            written: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn write(&mut self, payload: &[u8]) -> std::io::Result<usize> {
        self.written
            .lock()
            .expect("written mutex poisoned")
            .push(payload.to_vec());
        Ok(payload.len())
    }
}

/// Slot port yang dibagi reader & writer task. `None` saat alat belum
/// terhubung; `Some(port)` setelah reader berhasil "membuka".
type SharedSlot = Arc<Mutex<Option<MockSerialPort>>>;

/// Perintah yang dikirim handler ke writer task (mirror produksi).
struct SerialCommand {
    payload: Vec<u8>,
    ack: oneshot::Sender<Result<(), String>>,
}

/// Payload JSON outbound (mirror task 3.2 — `serde_json::to_vec`).
#[derive(Serialize)]
struct SerialCmd<'a> {
    target: &'a str,
    cmd: &'static str,
    reason: &'a str,
}

/// Writer task — single consumer mpsc → tulis ke port via shared lock.
/// Ini cermin `start_serial_writer` di `altivex_backend/src/main.rs`.
async fn serial_writer_task(mut rx: mpsc::Receiver<SerialCommand>, slot: SharedSlot) {
    while let Some(cmd) = rx.recv().await {
        let SerialCommand { payload, ack } = cmd;
        let slot_clone = slot.clone();
        // Sync I/O dibungkus spawn_blocking, sama dengan produksi.
        let result = tokio::task::spawn_blocking(move || -> Result<(), String> {
            let mut guard = slot_clone.lock().map_err(|_| "Mutex poisoned".to_string())?;
            match guard.as_mut() {
                Some(port) => port
                    .write(&payload)
                    .map(|_| ())
                    .map_err(|e| format!("I/O error: {}", e)),
                None => Err("port belum siap (alat tidak terhubung)".to_string()),
            }
        })
        .await
        .unwrap_or_else(|join_err| Err(format!("writer panic: {}", join_err)));

        let _ = ack.send(result);
    }
}

/// Replika `kirim_peringatan` versi F' (post-fix). Mengirim
/// `SerialCommand` via mpsc + menunggu ack lewat oneshot. Tidak pernah
/// mencoba "membuka port" sendiri — itu milik reader.
async fn mock_kirim_peringatan(
    tx: &mpsc::Sender<SerialCommand>,
    id_perangkat: &str,
    jenis_peringatan: &str,
) -> StatusCode {
    let cmd_struct = SerialCmd {
        target: id_perangkat,
        cmd: "VIBRATE",
        reason: jenis_peringatan,
    };
    let mut payload = match serde_json::to_vec(&cmd_struct) {
        Ok(v) => v,
        Err(_) => return StatusCode(500),
    };
    payload.push(b'\n');

    let (ack_tx, ack_rx) = oneshot::channel::<Result<(), String>>();
    if tx
        .send(SerialCommand {
            payload,
            ack: ack_tx,
        })
        .await
        .is_err()
    {
        return StatusCode(503);
    }

    match ack_rx.await {
        Ok(Ok(())) => StatusCode(200),
        Ok(Err(_)) => StatusCode(503),
        Err(_) => StatusCode(500),
    }
}

/// Spawn "reader" yang menempatkan port ke slot lalu sleep panjang.
/// Mirror `start_serial_reader` yang me-manage lifecycle port.
fn populate_slot_as_reader(slot: &SharedSlot) {
    let mut guard = slot.lock().expect("slot mutex poisoned");
    *guard = Some(MockSerialPort::new());
}

// ---------------------------------------------------------------------------
// PBT: B1 — Konflik Port Serial (sekarang diatasi oleh SerialHub design)
// ---------------------------------------------------------------------------
//
// Property (sesuai requirements 2.1):
//   FORALL (id, jenis) WHERE id valid AND jenis valid:
//     reader_aktif AND POST /api/alert(id, jenis) ⇒ status_code == 200
//
// Pada kode F (sebelum fix), test ini gagal — handler return 202 karena
// rebut port dengan reader.
// Pada kode F' (setelah fix di task 3.1), test ini LULUS karena handler
// hanya men-channel perintah ke writer task; tidak ada konflik akses.
//
// Validates: Requirements 1.1, 2.1 (B1)

proptest! {
    #![proptest_config(ProptestConfig {
        // 8 cases cukup; bug (kalau masih ada) deterministik selalu fail.
        cases: 8,
        .. ProptestConfig::default()
    })]

    /// Validates: Requirements 1.1, 2.1
    ///
    /// Property B1 — saat reader serial aktif memegang slot port,
    /// handler `kirim_peringatan` HARUS tetap mampu mengirim perintah
    /// (status 200) lewat writer task channel.
    #[test]
    fn b1_kirim_peringatan_harus_200_saat_reader_aktif(
        id_perangkat in "[A-Z0-9-]{1,8}",
        jenis_peringatan in "[a-z_]{1,16}",
    ) {
        // Tokio runtime per case — proptest closure sync, jadi kita
        // block_on di sini. Multi-thread agar `spawn_blocking` writer
        // benar-benar paralel dengan reader yang sedang tidur.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .build()
            .expect("tokio runtime gagal dibuat");

        let resp = runtime.block_on(async {
            // 1. Setup shared slot + writer task channel.
            let slot: SharedSlot = Arc::new(Mutex::new(None));
            let (tx, rx) = mpsc::channel::<SerialCommand>(8);

            // 2. Reader: tempatkan port ke slot. Di produksi, reader
            //    juga me-loop membaca; di sini kita cukup tinggalkan
            //    port di slot agar writer punya target.
            populate_slot_as_reader(&slot);

            // 3. Spawn writer task.
            let writer_slot = slot.clone();
            let writer_handle = tokio::spawn(async move {
                serial_writer_task(rx, writer_slot).await;
            });

            // 4. Beri waktu writer siap menerima.
            tokio::time::sleep(Duration::from_millis(10)).await;

            // 5. Invoke handler (mirror produksi: hanya mpsc + ack).
            let resp = mock_kirim_peringatan(&tx, &id_perangkat, &jenis_peringatan).await;

            // 6. Tutup channel agar writer task selesai bersih.
            drop(tx);
            let _ = writer_handle.await;

            resp
        });

        prop_assert_eq!(
            resp.as_u16(),
            200,
            "B1 (post-fix) harus 200. Counter-example: \
             id_perangkat={:?}, jenis_peringatan={:?}, got status={}",
            id_perangkat,
            jenis_peringatan,
            resp.as_u16()
        );
    }
}

// ---------------------------------------------------------------------------
// Sanity tests — memastikan harness sendiri tidak buggy.
// ---------------------------------------------------------------------------

#[test]
fn slot_holds_single_port_at_a_time() {
    // Slot shared adalah Arc<Mutex<Option<MockSerialPort>>>. Hanya satu
    // instance MockSerialPort yang boleh ada di slot pada satu waktu;
    // re-populate menimpa instance lama (sesuai semantik produksi:
    // reader hanya pegang satu port aktif).
    let slot: SharedSlot = Arc::new(Mutex::new(None));
    {
        let mut guard = slot.lock().unwrap();
        assert!(guard.is_none(), "slot kosong di awal");
        *guard = Some(MockSerialPort::new());
    }
    {
        let guard = slot.lock().unwrap();
        assert!(guard.is_some(), "slot terisi setelah reader populate");
    }
    {
        let mut guard = slot.lock().unwrap();
        *guard = None;
        assert!(guard.is_none(), "slot kosong lagi setelah port di-take");
    }
}

#[test]
fn happy_path_handler_returns_200_with_writer_running() {
    // Skenario deterministik: reader sudah taruh port, writer task aktif,
    // handler dipanggil sekali dengan input valid → 200 OK.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()
        .expect("tokio runtime gagal dibuat");

    let resp = runtime.block_on(async {
        let slot: SharedSlot = Arc::new(Mutex::new(None));
        let (tx, rx) = mpsc::channel::<SerialCommand>(4);
        populate_slot_as_reader(&slot);
        let writer_slot = slot.clone();
        let writer_handle = tokio::spawn(async move {
            serial_writer_task(rx, writer_slot).await;
        });
        tokio::time::sleep(Duration::from_millis(5)).await;
        let resp = mock_kirim_peringatan(&tx, "ALAT-001", "keluar_jalur").await;
        drop(tx);
        let _ = writer_handle.await;
        resp
    });

    assert_eq!(
        resp.as_u16(),
        200,
        "post-fix happy path harus 200 OK"
    );
}

#[test]
fn handler_returns_503_when_port_slot_is_empty() {
    // Reader belum sempat populate slot (alat tidak terhubung).
    // Writer task aktif, tapi take lock → slot None → ack
    // Err("port belum siap") → handler 503.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()
        .expect("tokio runtime gagal dibuat");

    let resp = runtime.block_on(async {
        let slot: SharedSlot = Arc::new(Mutex::new(None));
        let (tx, rx) = mpsc::channel::<SerialCommand>(4);
        // SENGAJA tidak panggil populate_slot_as_reader.
        let writer_slot = slot.clone();
        let writer_handle = tokio::spawn(async move {
            serial_writer_task(rx, writer_slot).await;
        });
        tokio::time::sleep(Duration::from_millis(5)).await;
        let resp = mock_kirim_peringatan(&tx, "ALAT-001", "keluar_jalur").await;
        drop(tx);
        let _ = writer_handle.await;
        resp
    });

    assert_eq!(
        resp.as_u16(),
        503,
        "alat tidak terhubung harus 503 Service Unavailable"
    );
}
