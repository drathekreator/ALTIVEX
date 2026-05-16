use actix_files::Files;
use actix_web::{web, App, HttpServer, Responder, HttpResponse, HttpRequest, Error};
use actix_web::web::Payload;
use sqlx::{postgres::PgPoolOptions, Pool, Postgres, FromRow};
use serde::{Deserialize, Serialize};
use dotenvy::dotenv;
use std::env;

use actix::{Actor, StreamHandler, AsyncContext, Handler, Message};
use actix_web_actors::ws;
use tokio::sync::broadcast;
use rumqttc::{AsyncClient, MqttOptions, QoS, Event, Packet};
use std::time::Duration;
use chrono::Utc;

// 1. Model data untuk menerima JSON dari perangkat (Heltec Basecamp)
#[derive(Deserialize, Serialize, Clone)]
struct IncomingData {
    id_perangkat: String,
    latitude: f64,
    longitude: f64,
}

// 2. Model data untuk dikirim ke Web Dashboard (diubah ke JSON)
#[derive(Serialize, FromRow)]
struct SensorRecord {
    id_perangkat: String,
    latitude: f64,
    longitude: f64,
}

// 3. Endpoint POST: Menyimpan data baru ke Database
async fn terima_data(
    data: web::Json<IncomingData>,
    pool: web::Data<Pool<Postgres>>,
    tx: web::Data<broadcast::Sender<String>>,
) -> impl Responder {
    let query = "
        INSERT INTO log_sensor (id_perangkat, latitude, longitude)
        VALUES ($1, $2, $3)
    ";

    // Mengeksekusi query insert ke PostgreSQL
    let result = sqlx::query(query)
        .bind(&data.id_perangkat)
        .bind(data.latitude)
        .bind(data.longitude)
        .execute(pool.get_ref())
        .await;

    // Broadcast data ke semua client WebSocket yang terhubung
    if let Ok(json_str) = serde_json::to_string(&*data) {
        let _ = tx.send(json_str);
    }

    match result {
        Ok(_) => HttpResponse::Ok().body("Berhasil: Data sensor tersimpan di Database!"),
        Err(e) => HttpResponse::InternalServerError().body(format!("Gagal menyimpan: {}", e)),
    }
}

// 4. Endpoint GET: Mengambil data terbaru untuk ditampilkan di Peta
async fn ambil_data(pool: web::Data<Pool<Postgres>>) -> impl Responder {
    // Menarik 50 data terbaru
    let query = "SELECT id_perangkat, latitude, longitude FROM log_sensor ORDER BY timestamp DESC LIMIT 50";

    let records = sqlx::query_as::<_, SensorRecord>(query)
        .fetch_all(pool.get_ref())
        .await;

    match records {
        Ok(data) => HttpResponse::Ok().json(data),
        Err(e) => HttpResponse::InternalServerError().body(format!("Gagal mengambil data: {}", e)),
    }
}

// Model data untuk History Path (Hanya Koordinat)
#[derive(Serialize, FromRow)]
struct HistoryRecord {
    latitude: f64,
    longitude: f64,
}

// Endpoint GET: Mengambil riwayat jalur perjalanan pendaki berdasarkan ID
async fn ambil_history(
    path: web::Path<String>,
    pool: web::Data<Pool<Postgres>>,
) -> impl Responder {
    let id_perangkat = path.into_inner();
    // Mengambil semua riwayat koordinat diurutkan dari yang paling lama ke terbaru
    let query = "SELECT latitude, longitude FROM log_sensor WHERE id_perangkat = $1 ORDER BY timestamp ASC";

    let records = sqlx::query_as::<_, HistoryRecord>(query)
        .bind(&id_perangkat)
        .fetch_all(pool.get_ref())
        .await;

    match records {
        Ok(data) => HttpResponse::Ok().json(data),
        Err(e) => HttpResponse::InternalServerError().body(format!("Gagal mengambil history: {}", e)),
    }
}

// Model data & Endpoint untuk Manajemen Pendaki (CRUD)
#[derive(Serialize, FromRow)]
struct Pendaki {
    id: i32,
    nama_pendaki: String,
    id_perangkat: String,
    telepon_darurat: String,
    tanggal_naik: chrono::NaiveDateTime,
    status: String,
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
async fn selesaikan_pendakian(path: web::Path<String>, pool: web::Data<Pool<Postgres>>) -> impl Responder {
    let id_perangkat = path.into_inner();
    let query = "UPDATE pendaki SET status = 'Sudah Turun' WHERE id_perangkat = $1 AND status = 'Mendaki'";
    let result = sqlx::query(query).bind(&id_perangkat).execute(pool.get_ref()).await;
    match result {
        Ok(_) => HttpResponse::Ok().body("Pendakian diselesaikan."),
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
async fn hapus_pendaki(path: web::Path<i32>, pool: web::Data<Pool<Postgres>>) -> impl Responder {
    let id = path.into_inner();
    let result = sqlx::query("DELETE FROM pendaki WHERE id = $1").bind(id).execute(pool.get_ref()).await;
    match result {
        Ok(_) => HttpResponse::Ok().body("Data pendaki dihapus."),
        Err(e) => HttpResponse::InternalServerError().body(format!("Error: {}", e)),
    }
}

// PUT /api/pendaki/{id} — Edit data pendaki
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
        Ok(_) => HttpResponse::Ok().body("Data pendaki diperbarui."),
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

// Endpoint POST: Meneruskan perintah ke Kabel USB (Serial)
async fn kirim_peringatan(req: web::Json<AlertRequest>) -> impl Responder {
    // 1. Format perintah menjadi JSON mini untuk dikirim lewat kabel Serial
    let command = format!(
        "{{\"target\":\"{}\", \"cmd\":\"VIBRATE\", \"reason\":\"{}\"}}\n",
        req.id_perangkat, req.jenis_peringatan
    );

    // 2. Simulasi Cetak ke Terminal (Bukti bahwa server merespons)
    println!("🚨 MENGIRIM PERINTAH DOWNLINK LORA 🚨");
    println!("Data Serial ke USB: {}", command);

    // 3. Logika Asli Serial Port (Dibungkus dengan Timeout agar tidak memblokir server selamanya)
    // Diatur dinamis lewat .env (misal COM3 untuk Windows, /dev/ttyUSB0 untuk Linux/WSL)
    let port_name = env::var("SERIAL_PORT").unwrap_or_else(|_| "/dev/ttyUSB0".to_string());
    let baud_rate = 115200;

    match serialport::new(&port_name, baud_rate)
        .timeout(std::time::Duration::from_millis(500))
        .open()
    {
        Ok(mut port) => match port.write(command.as_bytes()) {
            Ok(_) => {
                println!("✅ Berhasil dikirim ke Heltec Basecamp via Serial!");
                HttpResponse::Ok().body("Berhasil: Perintah peringatan diteruskan ke perangkat Basecamp!")
            }
            Err(e) => {
                println!("❌ Gagal menulis ke Serial: {:?}", e);
                HttpResponse::InternalServerError().body(format!("Gagal: I/O Serial Error: {}", e))
            }
        },
        Err(e) => {
            println!("⚠️ [SIMULASI] Perangkat Heltec belum terdeteksi di {}. Error: {:?}", port_name, e);
            HttpResponse::Accepted().body(format!("Simulasi: Perintah peringatan dicatat Server (Alat tidak terhubung di {}).", port_name))
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

// --- MQTT Client Logic ---
async fn start_mqtt_client(
    pool: Pool<Postgres>,
    tx: broadcast::Sender<String>,
) {
    let host = env::var("MQTT_BROKER_HOST").unwrap_or_else(|_| "localhost".to_string());
    let port = env::var("MQTT_BROKER_PORT").unwrap_or_else(|_| "1883".to_string()).parse::<u16>().unwrap_or(1883);
    
    let mut mqttoptions = MqttOptions::new("altivex_backend_cloud", host, port);
    mqttoptions.set_keep_alive(Duration::from_secs(5));

    let (client, mut eventloop) = AsyncClient::new(mqttoptions, 10);
    
    // Loop utama untuk retry koneksi jika Mosquitto sempat mati/restart
    loop {
        // Subscribe ke topik data sensor
        if let Err(e) = client.subscribe("altivex/sensor/data", QoS::AtMostOnce).await {
            println!("❌ Gagal subscribe MQTT: {:?}. Mencoba lagi dalam 5 detik...", e);
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue;
        }
        
        println!("📡 MQTT Subscriber aktif di topic: altivex/sensor/data");

        loop {
            match eventloop.poll().await {
                Ok(Event::Incoming(Packet::Publish(publish))) => {
                    let payload = publish.payload;
                    if let Ok(data) = serde_json::from_slice::<IncomingData>(&payload) {
                        // Simpan ke DB
                        let _ = sqlx::query("INSERT INTO log_sensor (id_perangkat, latitude, longitude) VALUES ($1, $2, $3)")
                            .bind(&data.id_perangkat)
                            .bind(data.latitude)
                            .bind(data.longitude)
                            .execute(&pool)
                            .await;

                        // Broadcast ke WebSocket
                        if let Ok(json_str) = serde_json::to_string(&data) {
                            let _ = tx.send(json_str);
                        }
                    }
                }
                Err(e) => {
                    println!("⚠️ MQTT Connection Error: {:?}. Reconnecting...", e);
                    break; // Keluar dari loop polling untuk masuk ke loop retry subscribe
                }
                _ => {}
            }
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

// --- Serial Reader Logic (Local Bridge / Failsafe) ---
async fn start_serial_reader(
    pool: Pool<Postgres>,
    tx: broadcast::Sender<String>,
) {
    let port_name = env::var("SERIAL_PORT").unwrap_or_else(|_| "COM3".to_string());
    let baud_rate = 115200;

    println!("🔌 Memulai Serial Reader di {}...", port_name);

    loop {
        match serialport::new(&port_name, baud_rate)
            .timeout(Duration::from_millis(1000))
            .open()
        {
            Ok(mut port) => {
                println!("✅ Terhubung ke Heltec Basecamp via Serial di {}", port_name);
                let mut serial_buf: Vec<u8> = vec![0; 1000];
                let mut line_buf = String::new();

                loop {
                    match port.read(serial_buf.as_mut_slice()) {
                        Ok(t) => {
                            let s = String::from_utf8_lossy(&serial_buf[..t]);
                            for c in s.chars() {
                                if c == '\n' {
                                    // Proses satu baris JSON
                                    if let Ok(data) = serde_json::from_str::<IncomingData>(&line_buf) {
                                        // 1. Simpan ke Local DB (jika ada)
                                        let _ = sqlx::query("INSERT INTO log_sensor (id_perangkat, latitude, longitude) VALUES ($1, $2, $3)")
                                            .bind(&data.id_perangkat)
                                            .bind(data.latitude)
                                            .bind(data.longitude)
                                            .execute(&pool)
                                            .await;

                                        // 2. Broadcast ke UI Lokal
                                        if let Ok(json_str) = serde_json::to_string(&data) {
                                            let _ = tx.send(json_str);
                                        }

                                        // 3. TODO: Di masa depan, kirim ke Cloud MQTT di sini jika internet aktif
                                        println!("📡 Data Serial diterima: {}", line_buf);
                                    }
                                    line_buf.clear();
                                } else if c != '\r' {
                                    line_buf.push(c);
                                }
                            }
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => (),
                        Err(e) => {
                            println!("❌ Error baca Serial: {:?}. Mencoba reconnect...", e);
                            break;
                        }
                    }
                }
            }
            Err(_) => {
                // Gagal buka port (mungkin alat tidak dicolok), tunggu dan coba lagi
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
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

    // MIGRASI: Tambahkan kolom telepon_darurat jika tabel lama belum punya
    let _ = sqlx::query("ALTER TABLE pendaki ADD COLUMN IF NOT EXISTS telepon_darurat VARCHAR(20) NOT NULL DEFAULT '';")
        .execute(&pool)
        .await;

    println!("✅ Database siap. Tabel log_sensor dan pendaki (dengan kolom telepon_darurat) tersedia.");

    // Jalankan MQTT Client di background
    let pool_mqtt = pool.clone();
    let tx_mqtt = tx.clone();
    tokio::spawn(async move {
        start_mqtt_client(pool_mqtt, tx_mqtt).await;
    });

    // Jalankan Serial Reader di background (Hanya untuk mode Lokal/Failsafe)
    let pool_serial = pool.clone();
    let tx_serial = tx.clone();
    tokio::spawn(async move {
        start_serial_reader(pool_serial, tx_serial).await;
    });

    println!("🚀 Server ALTIVEX berjalan di http://0.0.0.0:8080");

    // Mendaftarkan Endpoint (Routing URL)
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(tx_data.clone())
            .route("/api/sensor", web::post().to(terima_data))
            .route("/api/sensor", web::get().to(ambil_data))
            .route("/api/history/{id}", web::get().to(ambil_history))
            .route("/api/pendaki/riwayat", web::get().to(ambil_riwayat_pendaki))
            .route("/api/pendaki/cari", web::get().to(cari_pendaki))
            .route("/api/pendaki", web::get().to(ambil_pendaki))
            .route("/api/pendaki", web::post().to(registrasi_pendaki))
            .route("/api/pendaki/{id}", web::put().to(edit_pendaki))
            .route("/api/pendaki/{id}", web::delete().to(hapus_pendaki))
            .route("/api/pendaki/{id}/selesai", web::put().to(selesaikan_pendakian))
            .route("/api/alert", web::post().to(kirim_peringatan))
            .route("/api/status", web::get().to(cek_status))
            .route("/ws", web::get().to(ws_index))
            .service(Files::new("/", "./frontend").index_file("index.html"))
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}
