# Brief untuk AI Agent: Penyusunan Laporan Akhir ALTIVEX

> Dokumen ini adalah **brief lengkap** untuk dikirim ke AI agent lain
> (mis. ChatGPT, Claude, Gemini) yang akan menyusun laporan akhir
> proyek ALTIVEX. Brief disegmentasi per bab, dilengkapi knowledge
> base, dan instruksi khusus pencarian referensi akademik.
>
> **Cara pakai**: kirim Section A (konteks) ke agent, lalu Section B
> (instruksi global) lalu eksekusi Section C (prompt per bab) satu
> per satu. Agent akan output draft laporan setiap bab.

---

## Section A — Konteks Proyek (kirim sekali di awal)

> Salin-tempel blok di bawah sebagai pesan pertama ke AI agent.

```
Saya sedang menyusun laporan akhir proyek ALTIVEX. Anda akan saya
beri brief per bab. Sebelum mulai menulis, baca dan pahami konteks
proyek di bawah ini sebagai knowledge base.

═══════════════════════════════════════════════════════════════════
ALTIVEX — Knowledge Base
═══════════════════════════════════════════════════════════════════

NAMA PROYEK
ALTIVEX (Altitude + Tracker + Vex/Vexillum) — Sistem Pelacak
Pendaki Gunung Berbasis IoT dengan Geofencing dan Auto-Alert.

DOMAIN MASALAH
Setiap tahun banyak insiden pendaki gunung tersesat, terjebak di
luar jalur resmi, atau tidak terdeteksi dalam keadaan darurat di
Indonesia. Pos pendakian umumnya hanya mengandalkan buku tamu
manual dan radio HT. Saat pendaki tidak kembali sesuai jadwal,
operasi SAR baru dimulai berjam-jam kemudian, sering kali setelah
pendaki sudah jauh dari posisi terakhir yang diketahui. ALTIVEX
mengisi gap ini dengan sistem tracking real-time + alert otomatis
untuk penjaga pos.

ARSITEKTUR SISTEM (3 lapisan)

  Lapisan 1 — Edge Device (lapangan):
    - Device PENDAKI: ESP32 + GPS NEO-6M + modul LoRa
      (untuk produksi) atau Wi-Fi (untuk demo). Mengirim koordinat
      lat/lng + persen baterai tiap 5 detik
    - Device BASECAMP: ESP32 + buzzer aktif + tombol acknowledge
      fisik. Menerima alert otomatis dari server, membunyikan
      buzzer continuous saat ada alert aktif

  Lapisan 2 — Cloud Backend (server):
    - Bahasa: Rust (Actix-web framework)
    - Database: PostgreSQL 15
    - Message broker: Eclipse Mosquitto (MQTT v3.1.1) dengan
      autentikasi password, QoS 1
    - Reverse proxy: nginx + Let's Encrypt (TLS otomatis)
    - Deployment: Docker Compose di Google Cloud Platform
      Compute Engine
    - Geofence engine server-side: load file GeoJSON jalur,
      buffer 50m, evaluasi point-in-polygon tiap publish masuk

  Lapisan 3 — Web Dashboard (penjaga pos):
    - Frontend: HTML + JavaScript vanilla + Leaflet.js (peta) +
      Turf.js (komputasi geospasial client-side)
    - Real-time: WebSocket dari backend → push posisi pendaki
      ke peta tanpa refresh
    - Akses: HTTPS via domain (altivex-pangrango.duckdns.org
      untuk produksi, altivex-demo.duckdns.org untuk demo)

ALUR DATA UTAMA
1. Pendaki registrasi di pos: penjaga input nama, ID perangkat,
   nomor telepon darurat lewat dashboard
2. Pendaki bawa device, mendaki. Device kirim lat/lng tiap 5 detik
   via LoRa ke basecamp (atau Wi-Fi langsung di skenario demo)
3. Backend simpan ke tabel log_sensor PostgreSQL, broadcast ke
   semua dashboard yang terhubung via WebSocket
4. Dashboard render marker bergerak di peta + polyline jalur
5. Backend evaluasi 3 kondisi alert tiap publish:
   a. OUT_OF_GEOFENCE: posisi di luar koridor jalur (50m buffer)
   b. LOW_BATTERY: persen baterai < 15
   c. SIGNAL_LOST: pendaki status='Mendaki' tapi tidak ada
      publish baru lebih dari 10 menit
6. Saat alert ON, backend publish ke MQTT topic
   altivex/basecamp/cmd. Device basecamp subscribe topic ini,
   maintain set alert lokal, buzzer continuous selama set
   non-empty
7. Penjaga tekan tombol acknowledge fisik → buzzer silent
   (tetap tracking alert; alert baru re-arm buzzer)
8. Pendaki kembali ke koridor / baterai pulih / sinyal kembali
   → backend publish OFF → set di basecamp jadi kosong → buzzer
   mati otomatis
9. Pendaki kembali ke pos → penjaga klik "Selesai Pendakian"
   di dashboard

KEUNGGULAN TEKNIS YANG WORTH DI-HIGHLIGHT
- Geofencing server-side (independent dari browser)
- Property-Based Testing (PBT) dengan proptest crate untuk
  invariant validasi koordinat dan dedup
- Idempotent insert via UNIQUE INDEX (id_perangkat, timestamp)
  + ON CONFLICT DO NOTHING — handle MQTT QoS 1 retransmit
- Auto-reconnect: backend rebuild MQTT EventLoop saat error,
  exponential backoff. ESP32 watchdog Wi-Fi + MQTT
- Validasi koordinat ketat (reject NaN, out-of-range, (0,0)
  GPS lock-loss anomaly)
- TLS Let's Encrypt auto-renew via certbot

LOKASI DEMO DAN PRODUKSI
- Demo: loop bersepeda di kawasan Situgede, Bogor Barat
  (CIFOR → Jl. Cilubang Malang → Warung Tepi Hutan → CIFOR,
  ~2.7 km, ~10 menit)
- Produksi target: Taman Nasional Gunung Gede Pangrango,
  Jawa Barat (jalur Cibodas, Gunung Putri, Selabintana)

═══════════════════════════════════════════════════════════════════

Konfirmasi bahwa Anda sudah memahami konteks. Saya akan kirim
prompt per bab setelah ini.
```

---

## Section B — Instruksi Global Penyusunan Laporan

> Kirim ke AI agent setelah Section A dikonfirmasi.

```
Sebelum saya kirim brief per bab, ini aturan global penyusunan
laporan:

1. BAHASA DAN GAYA
   - Bahasa Indonesia akademik formal
   - Hindari first-person ("saya", "kami", "penulis")
   - Gunakan kalimat pasif untuk objektivitas
   - Tidak boleh menggunakan istilah marketing seperti "luar
     biasa", "revolusioner", "terbaik"

2. FORMAT
   - Bagian Latar Belakang, seluruh Tinjauan Pustaka, Kesimpulan,
     dan Saran ditulis dalam BENTUK PARAGRAF NARATIF
     (BUKAN bullet point atau tabel)
   - Bagian Rumusan Masalah, Tujuan, Manfaat, Analisis Kebutuhan,
     dan Fitur Utama BOLEH menggunakan bullet/numbered list
   - Setiap kutipan/klaim faktual yang membutuhkan referensi
     ditulis dengan format (Penulis, Tahun) di akhir kalimat
     atau paragraf
   - Sitasi gaya APA atau IEEE (pilih salah satu, konsisten)

3. REFERENSI — INI YANG PALING PENTING
   Untuk bagian yang memerlukan referensi (Latar Belakang dan
   seluruh Tinjauan Pustaka), Anda WAJIB:

   a. Mencari referensi akademik asli yang BENAR-BENAR ADA dan
      bisa diverifikasi (jurnal, prosiding konferensi, atau
      buku ilmiah). DILARANG mengarang nama penulis, judul
      jurnal, atau DOI. Lebih baik referensi sedikit tapi
      asli daripada banyak tapi fiktif.

   b. Hanya gunakan publikasi tahun 2021 sampai 2026
      (terbaru). Referensi lebih lama hanya boleh untuk
      definisi mendasar yang tidak berubah (mis. konsep IoT
      yang dirumuskan Atzori 2010 boleh, tapi state of the
      art harus 2021-2026)

   c. Domain pencarian yang dianjurkan:
      - IEEE Xplore (ieeexplore.ieee.org)
      - ScienceDirect (sciencedirect.com)
      - Springer Link (link.springer.com)
      - MDPI Sensors / Electronics / IoT (mdpi.com)
      - ACM Digital Library (dl.acm.org)
      - Jurnal nasional terindeks SINTA (Sinta 1-3) untuk
        konteks lokal Indonesia
      - Google Scholar boleh sebagai discovery tool, tapi
        verifikasi sumber asli sebelum dikutip

   d. Topik pencarian yang relevan:
      - "GPS hiker tracking system" (2021-2026)
      - "LoRa IoT mountain rescue"
      - "Geofencing real-time alert system"
      - "MQTT protocol IoT performance"
      - "WebSocket real-time dashboard"
      - "Hiker safety system Indonesia" (Bahasa Indonesia)
      - "Sistem pelacak pendaki" (Bahasa Indonesia)
      - "IoT geofencing trail"
      - "Rust web backend performance"
      - "PostgreSQL geospatial query"

   e. Untuk setiap referensi yang dikutip, sertakan di
      DAFTAR PUSTAKA dengan format APA lengkap:
        Penulis, A. B. (Tahun). Judul artikel. Nama Jurnal,
        Volume(Issue), halaman. https://doi.org/xxxxx

4. PANJANG NARASI
   - Latar Belakang: 4-6 paragraf (~600-900 kata)
   - Tinjauan Pustaka per sub-topik: 2-3 paragraf
   - Kesimpulan: 2-3 paragraf
   - Saran: 2-3 paragraf
   - Tidak bertele-tele, tiap paragraf punya argumen jelas

5. KONSISTENSI ISTILAH
   - "ALTIVEX" selalu kapital
   - "device pendaki" dan "device basecamp" (jangan
     "transmitter"/"receiver" karena ambigu)
   - "geofencing" tetap istilah Inggris (sudah lazim di IT)
   - "alert" tetap Inggris (lazim di IoT)
   - "pendaki" lebih cocok daripada "pengguna"
   - "penjaga pos" untuk operator dashboard

Konfirmasi pemahaman, lalu saya kirim prompt BAB I.
```

---

## Section C — Prompt Per Bab

### C.1 — Prompt BAB I PENDAHULUAN

```
Tulis BAB I PENDAHULUAN laporan akhir ALTIVEX dengan struktur:

1.1 LATAR BELAKANG
Bentuk: paragraf naratif, 4-6 paragraf.
Alur logika yang harus dibangun:
  Paragraf 1: konteks aktivitas pendakian gunung di Indonesia,
    statistik pendaki dan insiden (cari data dari jurnal atau
    laporan resmi BASARNAS / Kementerian Pariwisata 2021-2026)
  Paragraf 2: keterbatasan sistem pemantauan konvensional di pos
    pendakian (buku tamu manual, radio HT, ketergantungan pada
    waktu kembali yang dijadwalkan), sebut studi yang relevan
  Paragraf 3: peluang teknologi IoT untuk tracking pendaki —
    penurunan harga GPS, ketersediaan komunikasi LoRa untuk
    area remote, MQTT sebagai protokol ringan
  Paragraf 4: konsep geofencing dan real-time monitoring sebagai
    pendekatan yang lebih proaktif dibanding reaktif
  Paragraf 5: gap implementasi: sistem komersial mahal, tidak
    open, atau tidak terkalibrasi untuk topografi lokal
  Paragraf 6: posisikan ALTIVEX sebagai jawaban — sistem
    end-to-end, open, dengan auto-alert ke pos

WAJIB cari minimum 5 referensi akademik asli (2021-2026) untuk
bagian ini. Topik referensi yang dicari:
  - Statistik insiden pendakian Indonesia
  - Sistem pelacak pendaki berbasis IoT (general)
  - LoRa atau low-power wide area network untuk wilayah remote
  - Geofencing untuk safety monitoring
  - MQTT performance dalam IoT

1.2 RUMUSAN MASALAH
Bentuk: list bernomor, 3-5 poin pertanyaan masalah.
Contoh formulasi:
  1. Bagaimana merancang sistem pemantauan posisi pendaki secara
     real-time yang dapat beroperasi di area terbatas sinyal
     seluler?
  2. Bagaimana mendeteksi otomatis pendaki yang keluar dari jalur
     resmi tanpa intervensi manual penjaga pos?
  3. ... (lanjutkan sesuai konteks)

1.3 TUJUAN PROYEK
Bentuk: list bernomor, paralel dengan rumusan masalah.

1.4 MANFAAT
Bentuk: dua sub-bagian:
  - Manfaat akademik (kontribusi keilmuan, replikabilitas, dst.)
  - Manfaat praktis (untuk pengelola taman nasional, basecamp,
    pendaki, tim SAR)

OUTPUT: format markdown lengkap dengan heading sesuai struktur
laporan akhir akademik.
```

### C.2 — Prompt BAB II TINJAUAN PUSTAKA

```
Tulis BAB II TINJAUAN PUSTAKA laporan akhir ALTIVEX.

Bentuk: PARAGRAF NARATIF KESELURUHAN. Setiap sub-topik 2-3
paragraf. WAJIB referensi akademik asli (2021-2026) untuk SETIAP
sub-topik. Minimum total 12 referensi unik di bab ini.

Sub-topik yang harus dibahas (urutan ini):

2.1 INTERNET OF THINGS (IoT) DALAM SISTEM MONITORING
  Definisi IoT, arsitektur 3 lapis (perception, network,
  application), tren penggunaan IoT untuk safety monitoring
  outdoor 2021-2026

2.2 GLOBAL POSITIONING SYSTEM (GPS) DAN MODUL NEO-6M
  Prinsip kerja GPS, akurasi tipikal modul NEO-6M (umumnya
  2-5m), masalah lock loss di daerah hutan lebat dan strategi
  mitigasi. Sebut studi terbaru tentang akurasi GPS murah
  untuk aplikasi tracking

2.3 KOMUNIKASI LoRa UNTUK AREA TERPENCIL
  Karakteristik LoRa (long range, low power, sub-GHz),
  perbandingan dengan GSM dan Wi-Fi untuk area pegunungan,
  studi kasus implementasi LoRa di Indonesia atau negara
  tropis

2.4 PROTOKOL MQTT DAN BROKER MOSQUITTO
  Arsitektur publish-subscribe MQTT, jaminan QoS (0/1/2),
  keunggulan MQTT untuk perangkat berdaya rendah, peran
  broker (Mosquitto sebagai implementasi open source)

2.5 GEOFENCING DAN POINT-IN-POLYGON
  Konsep geofencing, algoritma point-in-polygon (ray casting),
  buffer LineString untuk koridor jalur, perbandingan
  evaluasi client-side vs server-side

2.6 BAHASA RUST DAN FRAMEWORK ACTIX-WEB
  Karakteristik Rust (memory safety tanpa garbage collection,
  zero-cost abstraction), Actix-web sebagai framework web
  async dengan performa tinggi, studi benchmarking 2021-2026

2.7 BASIS DATA POSTGRESQL UNTUK DATA SPATIO-TEMPORAL
  Kelebihan PostgreSQL untuk data deret-waktu (timestamp
  index), constraint UNIQUE INDEX untuk idempotency, aplikasi
  di sistem IoT logging

2.8 DOCKER DAN ORKESTRASI DENGAN DOCKER COMPOSE
  Prinsip containerization, isolation network, deployment
  reproducible. Kaitkan dengan praktik DevOps modern di IoT

2.9 PROPERTY-BASED TESTING (PBT)
  PBT versus unit testing, library proptest untuk Rust,
  manfaat PBT untuk validasi invariant input (mis. validasi
  koordinat geografis)

2.10 PENELITIAN TERKAIT (RELATED WORK)
  Bandingkan ALTIVEX dengan minimum 3 penelitian sejenis
  (sistem pelacak pendaki / safety IoT outdoor) yang
  diterbitkan 2021-2026. Jelaskan apa yang sama, apa yang
  beda, dan kontribusi unik ALTIVEX.

OUTPUT: markdown lengkap dengan heading 2.1 sampai 2.10. Sitasi
inline (Penulis, Tahun) di akhir setiap klaim faktual.

DILARANG mengarang referensi. Lebih baik 12 referensi asli yang
dapat diverifikasi daripada 30 referensi fiktif. Jika kesulitan
menemukan referensi untuk sub-topik tertentu, lebih baik akui
("studi spesifik dengan konteks ini masih terbatas") daripada
mengarang.
```

### C.3 — Prompt BAB III METODE PENELITIAN

```
Tulis BAB III METODE PENELITIAN laporan akhir ALTIVEX dengan
struktur:

3.1 ANALISIS KEBUTUHAN SISTEM
Bentuk: dua sub-tabel atau dua list bernomor.
  3.1.1 Kebutuhan Fungsional
    (apa yang sistem harus lakukan; mis. registrasi pendaki,
    push posisi, evaluasi geofence, dst.)
  3.1.2 Kebutuhan Non-Fungsional
    (performa, keamanan, ketersediaan, observabilitas)
  3.1.3 Kebutuhan Hardware
    Daftarkan: ESP32 (model spesifik), GPS NEO-6M, modul LoRa
    SX1276, buzzer aktif, push button, server cloud (vCPU,
    RAM, storage)
  3.1.4 Kebutuhan Software
    Daftarkan: Rust toolchain (versi 1.90+), PostgreSQL 15,
    Mosquitto 2, Docker, nginx, certbot. Untuk client/device:
    Arduino IDE 2.x, library PubSubClient, ArduinoJson,
    TinyGPSPlus

3.2 PERANCANGAN SISTEM
Bentuk: paragraf pengantar singkat, lalu sub-bagian dengan
diagram (deskripsikan textual, agent yang ngerender
gambar/diagram di Mermaid atau ASCII art).

  3.2.1 Arsitektur Tiga Lapisan
    (jelaskan edge / cloud / dashboard). Sertakan diagram
    sederhana dengan Mermaid syntax atau pseudographic.

  3.2.2 Skema Basis Data
    Jelaskan tabel `log_sensor` (id, id_perangkat, latitude,
    longitude, battery, timestamp) dan `pendaki` (id,
    nama_pendaki, id_perangkat, telepon_darurat,
    tanggal_naik, tanggal_turun, status). Sebut UNIQUE INDEX
    (id_perangkat, timestamp) untuk idempotency.

  3.2.3 Skema Komunikasi
    - Topic MQTT: altivex/sensor/data (uplink dari pendaki),
      altivex/basecamp/cmd (alert dari server ke basecamp),
      altivex/basecamp/ack (acknowledge dari basecamp ke
      server)
    - Schema payload sensor:
      {"id_perangkat":"...", "latitude":..., "longitude":...,
       "battery":...}
    - Schema payload alert:
      {"id_perangkat":"...", "kind":"OUT_OF_GEOFENCE|...",
       "state":"ON|OFF", "reason":"...", "nama_pendaki":"..."}

  3.2.4 Algoritma Geofencing
    Jelaskan langkah-langkah:
      1. Parse GeoJSON, ekstrak fitur LineString jalur
      2. Buffer LineString sebesar 50 meter (≈ 0.00045 derajat)
         menggunakan rectangle per segmen
      3. Gabungkan menjadi MultiPolygon
      4. Tiap publish posisi, evaluasi point-in-polygon
         (ray casting di crate geo)

  3.2.5 State Machine Auto-Alert
    Jelaskan transisi (Inactive → Active → Inactive) dengan
    debouncing per (id_perangkat, kategori). Sebut tiga
    kategori: OUT_OF_GEOFENCE, LOW_BATTERY, SIGNAL_LOST.

  3.2.6 Mekanisme Acknowledge Buzzer
    Tombol fisik di basecamp ESP32 men-silent buzzer namun
    tetap mempertahankan alert di set lokal. Alert baru
    masuk setelah silence akan re-arm buzzer.

3.3 DIAGRAM ALIR SISTEM
Bentuk: deskripsi diagram dalam Mermaid syntax atau
pseudographic. Buat 2-3 diagram alir:

  3.3.1 Alur Registrasi Pendaki
  3.3.2 Alur Tracking Posisi (uplink)
  3.3.3 Alur Auto-Alert dan Acknowledge

3.4 FITUR UTAMA
Bentuk: list bernomor dengan deskripsi 1-2 kalimat per fitur.
Daftarkan minimum 8 fitur, contoh:
  1. Real-time tracking — posisi pendaki update otomatis di
     peta tanpa refresh, latency < 1 detik
  2. Geofencing otomatis — alert keluar jalur tanpa
     intervensi manual operator
  3. Multi-device — banyak pendaki dapat dipantau bersamaan
  4. Dashboard berbasis web — diakses dari laptop atau HP
     pos pendakian
  5. Riwayat polyline — rekam jejak pendaki tersimpan,
     dapat di-export ke Excel
  6. Auto-alert baterai rendah — notifikasi saat baterai
     pendaki di bawah 15%
  7. Auto-alert sinyal hilang — notifikasi saat pendaki
     tidak terdeteksi lebih dari 10 menit
  8. Buzzer fisik di pos — penjaga tidak harus menatap layar
     terus-menerus
  9. Tombol acknowledge — penjaga dapat membungkamkan alert
     sementara tanpa menghapus tracking
  10. Otomatisasi clear — alert hilang sendiri saat kondisi
      pulih

OUTPUT: markdown lengkap dengan heading 3.1 sampai 3.4. Untuk
diagram, gunakan blok kode Mermaid.

Bab ini TIDAK perlu referensi akademik (kecuali saat menyebut
algoritma point-in-polygon, sebut sumber teori-nya).
```

### C.4 — Prompt BAB IV PENUTUP

```
Tulis BAB IV PENUTUP laporan akhir ALTIVEX dengan struktur:

4.1 KESIMPULAN
Bentuk: PARAGRAF NARATIF, 2-3 paragraf.
Tidak boleh bullet point.

Alur kesimpulan:
  Paragraf 1: ringkasan apa yang dicapai — sistem ALTIVEX
    berhasil dibangun end-to-end dari device sampai dashboard,
    dengan auto-alert tiga kategori. Sebut hasil pengujian:
    geofencing teruji di kawasan Situgede, polyline jalur
    snap-to-road, basecamp ESP32 menerima alert dan
    membunyikan buzzer secara otomatis.
  Paragraf 2: jawaban langsung untuk Rumusan Masalah di Bab I.
    Jika ada 4 rumusan masalah, bahas keempatnya secara
    ringkas.
  Paragraf 3: kontribusi ALTIVEX — sistem open-source dengan
    arsitektur dapat direplikasi untuk kawasan pendakian
    lain hanya dengan mengganti file GeoJSON.

4.2 SARAN
Bentuk: PARAGRAF NARATIF, 2-3 paragraf.
Tidak boleh bullet point.

Alur saran:
  Paragraf 1: pengembangan teknis langsung — integrasi modul
    LoRa untuk produksi penuh (saat ini demo masih pakai
    Wi-Fi), penambahan atribut altitude dari NEO-6M, panel
    surya untuk daya tahan device pendaki, dan TLS untuk
    MQTT
  Paragraf 2: pengembangan fitur — integrasi peta cuaca real
    time, alert prediktif berbasis machine learning untuk
    deteksi pola pergerakan abnormal, aplikasi mobile
    tertulang untuk SAR
  Paragraf 3: penerapan dan kebijakan — pilot project di
    Taman Nasional Gede Pangrango, kerjasama dengan
    BASARNAS dan pengelola taman, evaluasi kelayakan
    ekonomi untuk skala besar

OUTPUT: markdown lengkap dengan heading 4.1 dan 4.2.

Bab ini TIDAK memerlukan referensi akademik.
```

### C.5 — Prompt DAFTAR PUSTAKA

```
Setelah Bab I sampai IV selesai, kompilasi seluruh referensi
akademik yang sudah dikutip menjadi DAFTAR PUSTAKA.

Aturan:
1. Hanya tampilkan referensi yang BENAR-BENAR DIKUTIP di badan
   laporan (di-mention dengan format inline (Penulis, Tahun)).
   Jangan ada referensi orphan.
2. Format APA edisi 7. Contoh:
     Author, A. B., & Author, C. D. (2023). Judul artikel
       lengkap. Nama Jurnal, 12(3), 45-67.
       https://doi.org/10.xxxx/yyyy
3. Urutkan alfabetis berdasarkan nama keluarga penulis pertama.
4. Sertakan DOI atau URL valid.
5. Total minimum 17 referensi (5 dari Latar Belakang +
   minimum 12 dari Tinjauan Pustaka).
6. Semua harus tahun publikasi 2021-2026 KECUALI rujukan
   teori klasik yang masih relevan (mis. Atzori 2010 untuk
   definisi IoT).

DILARANG mengarang. Setiap entri harus dapat diverifikasi
melalui DOI atau URL aslinya.

OUTPUT: markdown plain list (tidak bernomor, tidak bullet,
sesuai standar APA).
```

---

## Section D — Catatan untuk Pengirim Brief

Saat AI agent selesai menulis tiap bab:

1. **Verifikasi referensi** — copy DOI yang dia berikan ke
   https://doi.org/, pastikan link benar-benar buka publikasi
   yang sesuai. Kalau DOI 404 atau judul tidak match, minta
   AI ganti referensi tersebut.

2. **Cross-check fakta teknis** — bandingkan deskripsi
   arsitektur di Bab III dengan kode aktual di repo (mis.
   ada gak sih `evaluate_alerts()` yang dia sebut). Kalau
   ada diskripansi, koreksi.

3. **Konsistensi istilah** — pastikan "ALTIVEX" konsisten
   kapital, dan dua device disebut "device pendaki" + "device
   basecamp" sepanjang dokumen.

4. **Panjang total laporan** — target 30-50 halaman A4 spasi
   1.5. Kalau terlalu pendek, minta agent expand bagian
   tertentu (biasanya Tinjauan Pustaka). Kalau terlalu panjang,
   minta ringkas (biasanya Latar Belakang yang berlebihan).

5. **Backup**: simpan output AI agent di file Markdown sebelum
   konversi ke Word/PDF — agar revisi mudah lewat git diff.

---

## Section E — Copy-Paste Cepat

Kalau mau langsung kirim full ke AI agent, gabungkan urutan ini:

1. Section A
2. Section B
3. Section C.1 → tunggu output → revisi kalau perlu
4. Section C.2 → tunggu output → revisi
5. Section C.3 → tunggu output → revisi
6. Section C.4 → tunggu output → revisi
7. Section C.5 → tunggu output → verifikasi DOI

Total estimasi 5-7 turn dengan AI agent untuk dokumen lengkap.
