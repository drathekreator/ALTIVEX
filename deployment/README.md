# ALTIVEX — Deploy ke GCP (post-`git pull` runbook)

Asumsi: VM GCP Compute Engine sudah ada, domain ter-pointing ke IP
eksternal VM, repo sudah di-clone via `git clone`.

> **Path penting**: GitHub repo Anda root-nya BUKAN di folder
> `altivex_backend/`. Setelah `git clone` di VM, semua file langsung
> ada di `~/ALTIVEX/` (bukan `~/ALTIVEX/altivex_backend/`). Semua
> perintah di runbook ini dijalankan dari `~/ALTIVEX/`.

Semua langkah dijalankan di VM **setelah** `git pull` di
`~/ALTIVEX/`.

---

## 0. Prasyarat (sekali per VM)

Jalankan di VM:

```bash
# Docker + Docker Compose v2 plugin
curl -fsSL https://get.docker.com | sudo sh
sudo usermod -aG docker "$USER"
# Logout + login ulang supaya group `docker` aktif tanpa sudo.

docker --version          # >= 24
docker compose version    # >= v2.20
```

Jalankan dari laptop Anda (`gcloud` lokal):

```bash
# Buka port 80 + 443 di firewall GCP
gcloud compute firewall-rules create altivex-http \
    --allow tcp:80,tcp:443 \
    --target-tags=http-server,https-server \
    --description="HTTP+HTTPS untuk dashboard ALTIVEX"

# Tag VM dengan rule di atas
gcloud compute instances add-tags <VM_NAME> \
    --zone=<ZONE> \
    --tags=http-server,https-server
```

---

## 1. First-time deploy

```bash
cd ~/ALTIVEX

# 1.a. Generate .env + mosquitto passwd otomatis dengan secret acak.
#      Idempotent — aman dijalankan ulang.
#      Kalau .env existing format lama, akan di-backup lalu regenerate.
bash deployment/bootstrap.sh
```

Output script akan menampilkan `API_AUTH_TOKEN` di terminal.
**Simpan token ini** — operator akan diminta paste saat buka
dashboard pertama kali.

```bash
# 1.b. Edit Caddyfile, ganti `altivex.example.com` ke domain Anda.
nano deployment/Caddyfile
```

Verifikasi DNS sudah propagate sebelum lanjut:

```bash
# Dari laptop Anda (bukan VM)
dig +short altivex.your-domain.com
# Harus return IP eksternal VM Anda.
```

```bash
# 1.c. Build + run produksi.
docker compose \
    -f docker-compose.yml \
    -f deployment/docker-compose.prod.yml \
    up -d --build
```

Build pertama kali: 5-10 menit (compile Rust release dengan deps
sqlx + actix). Build selanjutnya: 30-60 detik (Docker layer cache).

```bash
# 1.d. Cek logs.
docker compose logs -f backend
```

Yang harus muncul (4 baris kunci):

```
✅ Database siap. Tabel log_sensor dan pendaki ... tersedia.
📡 MQTT Subscriber aktif di topic: altivex/sensor/data (QoS=AtLeastOnce)
🔐 AuthMiddleware aktif untuk endpoint mutating.
🚀 Server ALTIVEX berjalan di http://0.0.0.0:8080
```

```bash
# 1.e. Cek Caddy issue cert Let's Encrypt.
docker compose logs -f caddy
```

Yang harus muncul:

```
certificate obtained successfully ... altivex.your-domain.com
```

---

## 2. Sanity test

Dari **laptop Anda** (bukan VM):

```bash
# Public — tidak butuh token
curl -i https://altivex.your-domain.com/api/status

# Mutating tanpa token → 401 Unauthorized
curl -i -X POST https://altivex.your-domain.com/api/sensor \
    -H "Content-Type: application/json" \
    -d '{"id_perangkat":"TEST","latitude":-6.7,"longitude":106.95}'

# Dengan token → 200 OK
curl -i -X POST https://altivex.your-domain.com/api/sensor \
    -H "Authorization: Bearer YOUR_API_TOKEN" \
    -H "Content-Type: application/json" \
    -d '{"id_perangkat":"TEST","latitude":-6.7,"longitude":106.95}'
```

Buka `https://altivex.your-domain.com/` di browser → muncul prompt
minta API token → paste token dari step 1.a.

---

## 3. Update setelah `git pull` berikutnya

```bash
cd ~/ALTIVEX
git pull

# Idempotent — TIDAK akan overwrite .env / passwd kalau sudah lengkap.
bash deployment/bootstrap.sh

# Build + restart (Postgres/Mosquitto zero-downtime, backend ~2 detik).
docker compose \
    -f docker-compose.yml \
    -f deployment/docker-compose.prod.yml \
    up -d --build

docker compose logs -f backend
```

---

## 4. Operasional

### Backup database (manual)

```bash
cd ~/ALTIVEX
docker compose exec postgres pg_dump \
    -U "$(grep POSTGRES_USER .env | cut -d= -f2)" \
    "$(grep POSTGRES_DB .env | cut -d= -f2)" \
    | gzip > "backup-$(date +%Y%m%d-%H%M%S).sql.gz"
```

### Backup database (cron tiap hari jam 02:00, retain 14 hari)

```bash
crontab -e
```

Tambah baris:

```
0 2 * * * cd ~/ALTIVEX && \
    docker compose exec -T postgres pg_dump \
    -U "$(grep POSTGRES_USER .env | cut -d= -f2)" \
    "$(grep POSTGRES_DB .env | cut -d= -f2)" \
    | gzip > "/var/backups/altivex-$(date +\%Y\%m\%d).sql.gz" && \
    find /var/backups -name 'altivex-*.sql.gz' -mtime +14 -delete
```

### Rotasi API token

```bash
cd ~/ALTIVEX
NEW_TOKEN=$(openssl rand -hex 32)
sed -i "s|^API_AUTH_TOKEN=.*|API_AUTH_TOKEN=$NEW_TOKEN|" .env

docker compose \
    -f docker-compose.yml \
    -f deployment/docker-compose.prod.yml \
    up -d backend

echo "New token: $NEW_TOKEN"
# Browser dashboard akan dapat 401 → otomatis prompt token baru.
```

### Rotasi password MQTT

```bash
cd ~/ALTIVEX
NEW_PWD=$(openssl rand -base64 18)
USER=$(grep MQTT_USERNAME .env | cut -d= -f2)

sed -i "s|^MQTT_PASSWORD=.*|MQTT_PASSWORD=$NEW_PWD|" .env

docker run --rm -v "$PWD/mosquitto/config:/work" -w /work \
    eclipse-mosquitto:2 \
    mosquitto_passwd -b passwd "$USER" "$NEW_PWD"

docker compose \
    -f docker-compose.yml \
    -f deployment/docker-compose.prod.yml \
    up -d mosquitto backend
```

### Monitor disk usage table `log_sensor`

```bash
cd ~/ALTIVEX
docker compose exec postgres psql \
    -U "$(grep POSTGRES_USER .env | cut -d= -f2)" \
    "$(grep POSTGRES_DB .env | cut -d= -f2)" \
    -c "SELECT pg_size_pretty(pg_total_relation_size('log_sensor'));"
```

---

## 5. Demo Branch (Multi-Gunung)

Untuk menjalankan instance ALTIVEX terpisah (peta berbeda, DB berbeda,
credential berbeda) di VM yang sama — misalnya untuk demo di gunung lain:

```bash
cd ~/ALTIVEX
bash deployment/demo-branch/bootstrap-demo.sh

docker compose -f deployment/demo-branch/docker-compose.demo.yml \
    --env-file deployment/demo-branch/.env.demo up -d --build
```

Detail lengkap: [`deployment/demo-branch/README.md`](demo-branch/README.md)

Port mapping default:
- Prod: backend `:8080`, MQTT `:1883`
- Demo: backend `:8081`, MQTT `:1884`

Tambah cabang lain (Semeru, Rinjani, dll.) dengan copy folder +
ganti port. Lihat panduan di README demo-branch.

---

## Troubleshooting

### `cd: No such file or directory`

Anda mungkin pakai path lama dari runbook sebelumnya
(`~/ALTIVEX/altivex_backend/`). GitHub repo root-nya = `~/ALTIVEX/`,
tidak ada subfolder `altivex_backend/`. Selalu `cd ~/ALTIVEX`.

### `bootstrap.sh: line ... unbound variable`

`.env` Anda format lama (kekurangan variable). Bootstrap script v2
sekarang auto-deteksi & backup ke `.env.backup-<timestamp>` lalu
regenerate. Pastikan Anda sudah `git pull` script yang terbaru:

```bash
cd ~/ALTIVEX
git pull
bash deployment/bootstrap.sh
```

### `error while interpolating ... POSTGRES_USER belum diset di .env`

Compose tidak baca `.env` — ada satu dari dua kemungkinan:

1. `.env` belum lengkap → jalankan ulang `bash deployment/bootstrap.sh`.
2. Anda jalankan `docker compose ...` dari folder yang BUKAN
   `~/ALTIVEX` (Compose hanya auto-load `.env` dari cwd). Pastikan
   `pwd` Anda = `~/ALTIVEX`.

### Caddy `[ERROR] obtain: ... no IP for hostname`

DNS belum propagate. Tunggu 5-15 menit, atau cek:

```bash
dig +short altivex.your-domain.com
# Harus return IP eksternal VM. Kalau kosong, A record belum dibuat
# atau TTL provider DNS-nya panjang.
```

### Caddy `[ERROR] obtain: ... acme: error 403 ... port 80 unreachable`

Firewall GCP belum buka port 80. Lihat step 0.

### Backend gagal connect ke Postgres saat first-startup

Postgres butuh > 30 detik untuk siap di VM kecil. Compose sudah
`depends_on: condition: service_healthy`, tapi healthcheck bisa
timeout. Workaround:

```bash
cd ~/ALTIVEX
docker compose \
    -f docker-compose.yml \
    -f deployment/docker-compose.prod.yml \
    up -d backend
```

### Heltec basecamp tidak terdeteksi

VM cloud TIDAK punya port serial fisik. Failsafe-mode (Serial
bridge) hanya relevan di hardware on-prem yang dicolok ke Heltec.
Untuk deploy GCP murni cloud, biarkan reader retry tiap 5 detik
tanpa perangkat — backend tidak crash, hanya log warning.
