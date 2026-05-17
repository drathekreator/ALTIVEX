# ALTIVEX — Deploy ke GCP (post-`git pull` runbook)

Asumsi: Anda sudah punya VM GCP (Compute Engine), domain ter-pointing
ke IP eksternal VM, dan repo sudah di-clone ke `~/ALTIVEX`.

Semua langkah di bawah dijalankan di VM, **setelah** `git pull` di
folder `~/ALTIVEX/altivex_backend/`.

---

## 0. Prasyarat (sekali per VM)

```bash
# Docker + Docker Compose v2 plugin
curl -fsSL https://get.docker.com | sudo sh
sudo usermod -aG docker "$USER"
# Logout + login ulang supaya group `docker` aktif tanpa sudo.

# Verifikasi
docker --version          # >= 24
docker compose version    # >= v2.20

# Firewall GCP (jalankan di gcloud lokal Anda, bukan VM):
gcloud compute firewall-rules create altivex-http \
    --allow tcp:80,tcp:443 \
    --target-tags=http-server,https-server \
    --description="HTTP+HTTPS untuk dashboard ALTIVEX"

# Tag VM (jalankan dari gcloud lokal Anda):
gcloud compute instances add-tags <VM_NAME> \
    --zone=<ZONE> \
    --tags=http-server,https-server
```

---

## 1. Setelah `git pull` — first-time bootstrap

```bash
cd ~/ALTIVEX/altivex_backend

# Buat .env + mosquitto passwd otomatis dengan secret acak.
# Idempotent: aman dijalankan ulang.
bash deployment/bootstrap.sh
```

Output script akan menampilkan `API_AUTH_TOKEN` di terminal. **Simpan
token ini** — operator akan diminta paste saat buka dashboard
pertama kali.

---

## 2. Edit Caddyfile — ganti domain

```bash
# Ganti `altivex.example.com` dengan domain Anda yang sudah
# punya A record ke IP VM ini.
nano deployment/Caddyfile
```

Verifikasi DNS sebelum lanjut:

```bash
dig +short altivex.your-domain.com
# Harus return IP eksternal VM Anda.
```

Caddy akan tolak issue cert Let's Encrypt kalau DNS belum propagate.

---

## 3. Build + run

```bash
docker compose \
    -f docker-compose.yml \
    -f deployment/docker-compose.prod.yml \
    up -d --build
```

Build pertama kali: 5-10 menit (compile Rust release dengan deps
sqlx + actix). Build selanjutnya: 30-60 detik (Docker cache layer
deps).

---

## 4. Verifikasi

```bash
# Status semua container
docker compose ps

# Logs backend (tunggu sampai lihat 4 baris ini):
#   ✅ Database siap.
#   📡 MQTT Subscriber aktif di topic: altivex/sensor/data (QoS=AtLeastOnce)
#   🔐 AuthMiddleware aktif untuk endpoint mutating.
#   🚀 Server ALTIVEX berjalan di http://0.0.0.0:8080
docker compose logs -f backend

# Logs Caddy (tunggu sampai lihat cert obtained):
#   certificate obtained successfully ... altivex.your-domain.com
docker compose logs -f caddy
```

Sanity test endpoint dari laptop Anda (bukan VM):

```bash
# Public — tidak butuh token
curl -i https://altivex.your-domain.com/api/status

# Mutating tanpa token → expect 401
curl -i -X POST https://altivex.your-domain.com/api/sensor \
    -H "Content-Type: application/json" \
    -d '{"id_perangkat":"TEST","latitude":-6.7,"longitude":106.95}'

# Dengan token → expect 200
curl -i -X POST https://altivex.your-domain.com/api/sensor \
    -H "Authorization: Bearer $YOUR_API_TOKEN" \
    -H "Content-Type: application/json" \
    -d '{"id_perangkat":"TEST","latitude":-6.7,"longitude":106.95}'
```

Buka `https://altivex.your-domain.com/` di browser. Akan muncul
prompt minta API token — paste token dari step 1.

---

## 5. Update setelah `git pull` berikutnya

```bash
cd ~/ALTIVEX/altivex_backend
git pull

# Bootstrap idempotent — tidak akan overwrite .env atau passwd.
bash deployment/bootstrap.sh

# Build + restart (zero downtime untuk Postgres/Mosquitto, backend
# akan restart ~2 detik).
docker compose \
    -f docker-compose.yml \
    -f deployment/docker-compose.prod.yml \
    up -d --build

docker compose logs -f backend
```

---

## 6. Operasional

### Backup database

```bash
docker compose exec postgres pg_dump \
    -U "$(grep POSTGRES_USER .env | cut -d= -f2)" \
    "$(grep POSTGRES_DB .env | cut -d= -f2)" \
    | gzip > "backup-$(date +%Y%m%d-%H%M%S).sql.gz"
```

Schedule via cron:

```bash
# Edit crontab
crontab -e

# Tambah baris (backup tiap hari jam 02:00, simpan 14 hari terakhir)
0 2 * * * cd ~/ALTIVEX/altivex_backend && \
    docker compose exec -T postgres pg_dump \
    -U "$(grep POSTGRES_USER .env | cut -d= -f2)" \
    "$(grep POSTGRES_DB .env | cut -d= -f2)" \
    | gzip > "/var/backups/altivex-$(date +\%Y\%m\%d).sql.gz" && \
    find /var/backups -name 'altivex-*.sql.gz' -mtime +14 -delete
```

### Rotasi API token

```bash
NEW_TOKEN=$(openssl rand -hex 32)
sed -i "s|^API_AUTH_TOKEN=.*|API_AUTH_TOKEN=$NEW_TOKEN|" .env

docker compose \
    -f docker-compose.yml \
    -f deployment/docker-compose.prod.yml \
    up -d backend

echo "New token: $NEW_TOKEN"
# Operator dashboard akan dapat 401 → otomatis di-prompt token baru.
```

### Rotasi password MQTT

```bash
NEW_PWD=$(openssl rand -base64 18)
USER=$(grep MQTT_USERNAME .env | cut -d= -f2)

# Update .env
sed -i "s|^MQTT_PASSWORD=.*|MQTT_PASSWORD=$NEW_PWD|" .env

# Regenerate passwd
docker run --rm -v "$PWD/mosquitto/config:/work" -w /work \
    eclipse-mosquitto:2 \
    mosquitto_passwd -b passwd "$USER" "$NEW_PWD"

# Restart broker + backend
docker compose \
    -f docker-compose.yml \
    -f deployment/docker-compose.prod.yml \
    up -d mosquitto backend
```

### Monitor disk usage

```bash
# Cek growth log_sensor
docker compose exec postgres psql \
    -U "$(grep POSTGRES_USER .env | cut -d= -f2)" \
    "$(grep POSTGRES_DB .env | cut -d= -f2)" \
    -c "SELECT pg_size_pretty(pg_total_relation_size('log_sensor'));"

# Kalau growth terlalu besar, tambah retention policy via pg_cron
# atau cron job manual yang DELETE WHERE timestamp < NOW() - interval.
```

---

## Troubleshooting

### Caddy tidak bisa issue cert

```
[ERROR] obtain: ... no IP for hostname
```

DNS belum propagate. Tunggu 5-15 menit, atau cek `dig +short
altivex.your-domain.com` di laptop Anda.

```
[ERROR] obtain: ... acme: error 403 ... port 80 unreachable
```

Firewall GCP belum buka port 80. Lihat step 0.

### Backend exit dengan "X belum diset di .env"

`.env` hilang atau tidak ter-baca. Re-run bootstrap:

```bash
ls -la .env  # harus ada
bash deployment/bootstrap.sh
```

### MQTT log "bad credentials"

`mosquitto/config/passwd` tidak match `MQTT_USERNAME`/`MQTT_PASSWORD`
di `.env`. Regenerate:

```bash
USER=$(grep MQTT_USERNAME .env | cut -d= -f2)
PWD=$(grep MQTT_PASSWORD .env | cut -d= -f2)
docker run --rm -v "$PWD/mosquitto/config:/work" -w /work \
    eclipse-mosquitto:2 \
    mosquitto_passwd -b -c passwd "$USER" "$PWD"
docker compose restart mosquitto
```

### Backend gagal connect ke Postgres saat first-startup

Postgres belum siap saat backend coba connect. Compose sudah punya
`depends_on: condition: service_healthy`, tapi kalau Postgres butuh
> 30 detik (mis. VM kecil), backend bisa exit. Jalankan ulang:

```bash
docker compose \
    -f docker-compose.yml \
    -f deployment/docker-compose.prod.yml \
    up -d backend
```

### Heltec basecamp tidak terdeteksi

VM cloud TIDAK punya port serial fisik. Failsafe-mode (Serial
bridge) hanya relevan kalau Anda deploy di hardware on-prem
(Raspberry Pi / mini-PC) yang dicolok ke Heltec. Untuk deploy GCP
murni cloud, biarkan reader retry tiap 5 detik tanpa perangkat —
tidak akan crash, hanya log warning.

Kalau Anda butuh failsafe Serial DI VM cloud, pakai serial-over-IP
(ser2net + socat) — tapi itu di luar scope deploy ini.
