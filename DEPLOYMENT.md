# ALTIVEX — Deployment Guide

Dokumen ini wajib dibaca operator sebelum:

1. Push ke GitHub publik (publish gate).
2. Build + run container produksi (deploy gate).

Ada beberapa langkah satu-kali yang harus dilakukan agar repo aman
dan stack berjalan tanpa silent failure.

---

## Pre-publish gate (sebelum push ke GitHub)

### 1. Rotasi credential lama yang sempat bocor di git history

`altivex_backend/.env` pernah ter-commit di commit `a3f41c2`
(Initial commit) dan dihapus di `78adfd9`. Password di file itu
(`secretpassword`) sudah ada di history publik. Sebelum publish:

- [ ] **Ganti password Postgres produksi**. Password lama jangan
  dipakai lagi di environment apa pun (dev / staging / prod).
- [ ] **Ganti password MQTT produksi**. Regenerate
  `mosquitto/config/passwd` (lihat bagian deploy di bawah).
- [ ] **Generate `API_AUTH_TOKEN` baru** dengan `openssl rand -hex 32`.

Optional (tergantung policy keamanan Anda): lakukan history rewrite
untuk benar-benar menghilangkan password lama dari history publik.
Tools yang umum dipakai:

- `git filter-repo` (Recommended,
  <https://github.com/newren/git-filter-repo>):
  ```bash
  git filter-repo --path altivex_backend/.env --invert-paths
  ```
- BFG Repo-Cleaner.

History rewrite akan mengubah seluruh commit hash setelah commit
yang di-rewrite. Setiap kontributor harus `git fetch --all` +
`git reset --hard origin/main` setelah rewrite.

### 2. Pastikan tidak ada secret yang akan ke-commit

```bash
cd altivex_backend
git status --short
```

File-file ini WAJIB tidak ter-track:

- `.env` (dilindungi `.gitignore`).
- `mosquitto/config/passwd` (dilindungi `.gitignore`).
- `target/` (dilindungi `.gitignore`).

Sanity check: `git ls-files | grep -E "(\.env$|passwd$|target)"`
harus tidak mengembalikan apa pun (kecuali `.env.example`).

### 3. Bersihkan workspace-root `docker-compose.yml`

File `c:\Users\USER\Documents\ALTIVEX\docker-compose.yml` (di luar
repo `altivex_backend/`) berisi password literal `secretpassword`.
Karena di luar repo, dia tidak akan ke-commit, tapi tetap jadi
risiko leak via screenshot / sharing. Hapus password literal-nya
dan ganti ke `${POSTGRES_PASSWORD:?...}` (mirror compose
production), atau hapus saja file itu kalau memang tidak dipakai.

### 4. Verifikasi build + test

```bash
# Rust
cargo build --release
cargo test

# Frontend
cd frontend
npx vitest run
```

Semua harus hijau. Saat ini:

- Rust: 11/11 (4 exploration + 7 preservation)
- Frontend: 8/8 (2 exploration + 6 preservation)

---

## Pre-deploy gate (sebelum `docker compose up -d`)

### 1. Generate `.env`

```bash
cp .env.example .env
```

Isi setiap `REPLACE_ME_*`:

- `DATABASE_URL`: `postgres://<USER>:<PWD>@postgres:5432/altivex_db`.
  `USER` + `PWD` harus match `POSTGRES_USER` + `POSTGRES_PASSWORD`.
- `POSTGRES_PASSWORD`: pakai `openssl rand -base64 24`.
- `MQTT_USERNAME`: misal `altivex_prod`.
- `MQTT_PASSWORD`: pakai `openssl rand -base64 24`.
- `API_AUTH_TOKEN`: `openssl rand -hex 32`.

### 2. Generate `mosquitto/config/passwd`

Wajib match `MQTT_USERNAME` + `MQTT_PASSWORD` di `.env`:

```bash
cd altivex_backend
docker run --rm -v "$PWD/mosquitto/config":/work \
    -w /work eclipse-mosquitto:2 \
    mosquitto_passwd -b -c passwd "$MQTT_USERNAME" "$MQTT_PASSWORD"

chmod 0600 mosquitto/config/passwd  # Linux/macOS
```

Verifikasi:

```bash
cat mosquitto/config/passwd
# Harus menampilkan baris: <username>:$7$...<hash>...
```

### 3. Build + run

```bash
docker compose up -d --build
docker compose logs -f backend
```

Logs yang diharapkan:

- `✅ Database siap.`
- `📡 MQTT Subscriber aktif di topic: altivex/sensor/data (QoS=AtLeastOnce)`
- `🔐 AuthMiddleware aktif untuk endpoint mutating.`
- `🚀 Server ALTIVEX berjalan di http://0.0.0.0:8080`

Kalau backend exit dengan error `API_AUTH_TOKEN belum diset`,
artinya compose `.env` tidak terbaca atau token kosong.

### 4. Sanity test endpoint

```bash
# Public endpoint (tidak perlu token)
curl http://localhost:8080/api/status

# Mutating endpoint TANPA token → expect 401
curl -X POST http://localhost:8080/api/sensor \
    -H "Content-Type: application/json" \
    -d '{"id_perangkat":"TEST","latitude":-6.7,"longitude":106.95}'

# Dengan token → expect 200 + 1 row di log_sensor
curl -X POST http://localhost:8080/api/sensor \
    -H "Authorization: Bearer <YOUR_TOKEN>" \
    -H "Content-Type: application/json" \
    -d '{"id_perangkat":"TEST","latitude":-6.7,"longitude":106.95}'
```

### 5. Reverse proxy (nginx)

Compose ini sengaja TIDAK menyertakan TLS. Deploy nginx (atau
Traefik / Caddy) di luar compose untuk:

- TLS termination (Let's Encrypt).
- Routing `/api/*` + `/ws` ke `backend:8080`.
- Routing `/` ke static index (atau langsung ke backend yang
  sudah serve frontend dari `Files::new("/", "./frontend")`).
- Optional: rate limiting di `/api/sensor` dan `/api/alert`.

---

## Operasional

### Update password MQTT setelah produksi hidup

```bash
# 1. Generate hash baru
docker run --rm -v "$PWD/mosquitto/config":/work \
    -w /work eclipse-mosquitto:2 \
    mosquitto_passwd -b passwd <username> <new_password>

# 2. Update .env dengan password baru
# 3. Restart broker + backend (broker baca passwd file, backend
#    baca env)
docker compose restart mosquitto backend
```

### Update `API_AUTH_TOKEN` (rotasi rutin)

```bash
# 1. Generate token baru
NEW_TOKEN=$(openssl rand -hex 32)

# 2. Update .env
sed -i "s|^API_AUTH_TOKEN=.*|API_AUTH_TOKEN=$NEW_TOKEN|" .env

# 3. Restart backend
docker compose up -d backend

# 4. Browser dashboard akan dapat 401 → otomatis prompt token baru
#    (lihat dashboard.js → apiFetch retry handler).
```

### Backup database

```bash
docker compose exec postgres pg_dump -U "$POSTGRES_USER" "$POSTGRES_DB" \
    | gzip > backup-$(date +%Y%m%d-%H%M%S).sql.gz
```

---

## Troubleshooting

### `connection refused` ke MQTT setelah restart

Kemungkinan broker belum siap. Tunggu 5 detik atau cek
`docker compose logs mosquitto`. Backend punya exponential backoff
1s → 2s → 4s → ... → 30s capped, jadi akan reconnect sendiri.

### `401 Unauthorized` dari frontend

Token di `localStorage["ALTIVEX_API_TOKEN"]` salah / lawas. Buka
DevTools → Application → Local Storage → hapus key tsb → reload.
Frontend akan prompt token baru.

### Banner alert "X pendaki di luar koridor" tidak muncul

Cek di browser DevTools console:

- `geofenceBuffer` undefined → `GEO.json` gagal load (cek
  Network tab).
- `outsideCount === 0` selalu meskipun pendaki real keluar buffer
  → cek polygon `geofenceBuffer` covers area pendakian.

### Serial reader spam log "Error baca Serial: TimedOut"

Normal saat Heltec idle (tidak kirim data). Reader retry baca
tiap iterasi. Kalau alat benar-benar dicabut, log akan jadi
`Error baca Serial: NotPresent` dan reader akan masuk loop
reconnect 5s.
