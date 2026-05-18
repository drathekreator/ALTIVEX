# Brief untuk AI Agent — Project Kedua di VM yang Sama

> Salin file ini ke awal percakapan dengan agent AI saat mulai project
> baru di VM yang sudah menjalankan ALTIVEX. Agent harus baca seluruh
> dokumen ini SEBELUM membuat compose / nginx / firewall apa pun.

## 1. Konteks VM yang sudah berjalan

```
VM       : GCP instance-20260424-035716 (asia-southeast2-a)
IP       : 35.219.1.25
Domain   : altivex-pangrango.duckdns.org  (DuckDNS, gratis multi-subdomain)
OS       : Ubuntu/Debian + Docker + Docker Compose v2
User     : Indra (sudo)
Repo     : ~/ALTIVEX/  (root = altivex_backend, github.com/drathekreator/ALTIVEX)
```

Service ALTIVEX yang AKTIF (DO NOT TOUCH):

| Komponen        | Container         | Port host       | Network             |
|-----------------|-------------------|-----------------|---------------------|
| Backend (Rust)  | `altivex_backend` | 8080→8080       | `altivex_default`   |
| Postgres 15     | `altivex_postgres`| internal only   | `altivex_default`   |
| Mosquitto 2     | `altivex_mosquitto`| 1883→1883      | `altivex_default`   |
| nginx (host)    | systemd           | 80, 443         | host                |

DNS: `altivex-pangrango.duckdns.org` → `35.219.1.25` (A record)
TLS: Let's Encrypt via certbot (auto-renew aktif)
GCP firewall rules:
- `default-allow-http` (80)
- `default-allow-https` (443)
- `altivex-mqtt` (1883, target tag `mqtt-broker`)

## 2. Surface yang TIDAK BOLEH bentrok

Project kedua **WAJIB** menghindari hal berikut:

### Port host (yang sudah dipakai)
- 80, 443  → nginx
- 1883     → mosquitto ALTIVEX
- 8080     → backend ALTIVEX

### Nama Docker (yang sudah dipakai)
- container: `altivex_backend`, `altivex_postgres`, `altivex_mosquitto`
- network: `altivex_default`
- volume: `altivex_pgdata`

### Path filesystem
- `~/ALTIVEX/` → repo ALTIVEX
- `/etc/nginx/sites-enabled/altivex` → vhost ALTIVEX
- `/etc/letsencrypt/live/altivex-pangrango.duckdns.org/` → cert ALTIVEX

## 3. Konvensi WAJIB untuk project kedua

Misal nama project = `<proj>` (contoh: `meteora`, `voka`, `dst`).

### Naming
- Repo path: `~/<proj>/`
- Container: `<proj>_backend`, `<proj>_postgres`, `<proj>_mosquitto` (jika butuh)
- Network: `<proj>_default` (set eksplisit di compose)
- Volume: `<proj>_pgdata`, dst — selalu prefix `<proj>_`
- Subdomain: `<proj>.duckdns.org` (daftar gratis di duckdns.org)
- Nginx vhost: `/etc/nginx/sites-enabled/<proj>`

### Port host
- HTTP/HTTPS publik: **JANGAN publish dari container**. Routing lewat nginx
  host yang sudah ada. Backend cukup `expose: ["8080"]` (tanpa `ports:`).
- Backend container internal port: pilih `<port>` yang berbeda dari 8080.
  Saran: 8081, 8082, dst.
- Postgres: jangan publish ke host. Backend akses via `<proj>_postgres:5432`
  di network internal.

### MQTT — pilih SALAH SATU strategi (lihat §4)

## 4. Strategi MQTT (penting — pilih sebelum coding)

### Opsi A — Pakai broker ALTIVEX (shared broker, hemat resource)

**Kapan dipakai:** project kedua trafik MQTT rendah, tidak ada concern
isolasi keras antara device dua project.

**Yang perlu dilakukan agent:**

1. **Tambah user MQTT baru** di mosquitto ALTIVEX:
   ```bash
   cd ~/ALTIVEX
   docker compose exec mosquitto mosquitto_passwd \
       -b /mosquitto/config/passwd <proj>_user <password_baru>
   docker compose restart mosquitto
   ```

2. **Aktifkan ACL** di `mosquitto/config/mosquitto.conf` (kalau belum):
   ```
   acl_file /mosquitto/config/acl
   ```

3. **Tambah ACL** di `mosquitto/config/acl`:
   ```
   user altivex
   topic readwrite altivex/#

   user <proj>_user
   topic readwrite <proj>/#
   ```

4. **Topic prefix WAJIB** di project kedua: semua topic `<proj>/...`,
   contoh `<proj>/sensor/data`. JANGAN sekali-kali publish/subscribe
   ke `altivex/#`.

5. **Backend project kedua** join `altivex_default` network supaya bisa
   resolve `altivex_mosquitto:1883`:
   ```yaml
   # docker-compose.yml project kedua
   networks:
     default:
       name: <proj>_default
     altivex_shared:
       external: true
       name: altivex_default

   services:
     backend:
       networks:
         - default          # untuk DB project sendiri
         - altivex_shared   # untuk akses ke broker ALTIVEX
       environment:
         MQTT_BROKER_HOST: altivex_mosquitto
         MQTT_BROKER_PORT: 1883
         MQTT_USERNAME: <proj>_user
         MQTT_PASSWORD: <isi dari .env>
   ```

### Opsi B — Broker terpisah per project (isolasi penuh)

**Kapan dipakai:** project kedua punya banyak device, butuh isolasi
keras, atau policy berbeda (mis. anonymous, TLS-only, dst).

**Yang perlu dilakukan agent:**

1. Compose project kedua punya service `mosquitto` sendiri:
   ```yaml
   services:
     mosquitto:
       image: eclipse-mosquitto:2
       container_name: <proj>_mosquitto
       expose: ["1883"]
       # publish ke host kalau device perlu konek dari internet:
       ports:
         - "1884:1883"   # 1884 = pilih port yang BELUM dipakai
   ```

2. **Buka GCP firewall** untuk port baru:
   ```bash
   gcloud compute firewall-rules create <proj>-mqtt \
       --allow tcp:1884 \
       --target-tags=mqtt-broker
   ```

3. Jangan ada container yang join `altivex_default` — keep isolated.

## 5. Postgres

ALWAYS broker terpisah per project (isolasi data wajib):

```yaml
services:
  postgres:
    image: postgres:15-alpine
    container_name: <proj>_postgres
    environment:
      POSTGRES_USER: ${POSTGRES_USER:?}
      POSTGRES_PASSWORD: ${POSTGRES_PASSWORD:?}
      POSTGRES_DB: ${POSTGRES_DB:?}
    # JANGAN publish 5432 ke host
    volumes:
      - <proj>_pgdata:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U $${POSTGRES_USER}"]

volumes:
  <proj>_pgdata:
```

`.env` project kedua punya secret yang **berbeda** dari ALTIVEX. Jangan
salin password ALTIVEX. Generate baru:
```bash
openssl rand -hex 32   # API_AUTH_TOKEN
openssl rand -hex 16   # POSTGRES_PASSWORD (URL-safe untuk DATABASE_URL)
```

## 6. nginx integration

Buat vhost baru di host:

```nginx
# /etc/nginx/sites-available/<proj>
server {
    listen 80;
    listen [::]:80;
    server_name <proj>.duckdns.org;

    location /.well-known/acme-challenge/ {
        root /var/www/certbot;
        try_files $uri =404;
    }

    location / {
        return 301 https://$host$request_uri;
    }
}

server {
    listen 443 ssl http2;
    listen [::]:443 ssl http2;
    server_name <proj>.duckdns.org;

    ssl_certificate     /etc/letsencrypt/live/<proj>.duckdns.org/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/<proj>.duckdns.org/privkey.pem;
    ssl_protocols       TLSv1.2 TLSv1.3;

    add_header Strict-Transport-Security "max-age=31536000; includeSubDomains" always;
    add_header X-Content-Type-Options "nosniff" always;

    client_max_body_size 8m;

    access_log /var/log/nginx/<proj>.access.log;
    error_log  /var/log/nginx/<proj>.error.log warn;

    location / {
        proxy_pass http://127.0.0.1:<port_baru>;   # port internal backend project
        proxy_http_version 1.1;
        proxy_set_header Host              $host;
        proxy_set_header X-Real-IP         $remote_addr;
        proxy_set_header X-Forwarded-For   $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # WebSocket upgrade (kalau project kedua pakai WS)
        proxy_set_header Upgrade    $http_upgrade;
        proxy_set_header Connection $connection_upgrade;
        proxy_cache_bypass          $http_upgrade;

        proxy_read_timeout 3600s;
    }
}
```

> Catatan: `map $http_upgrade $connection_upgrade {...}` SUDAH didefinisikan
> di vhost ALTIVEX (di luar server block, scope `http`). Cukup di-share —
> jangan duplikasi map block atau nginx error "duplicate map".

Aktifkan:
```bash
sudo ln -s /etc/nginx/sites-available/<proj> /etc/nginx/sites-enabled/
sudo nginx -t
sudo systemctl reload nginx

# Issue cert (setelah DNS A record <proj>.duckdns.org → 35.219.1.25 aktif)
sudo certbot --nginx -d <proj>.duckdns.org
```

Tapi project kedua harus **publish port internal backend ke host** supaya
nginx bisa hit `127.0.0.1:<port_baru>`:

```yaml
backend:
  ports:
    - "127.0.0.1:<port_baru>:<port_baru>"   # bind localhost saja
```

`127.0.0.1:` bind penting — supaya port tidak terbuka ke internet (cuma
nginx host yang bisa hit).

## 7. DuckDNS — register subdomain baru

1. Login ke duckdns.org pakai akun yang sama dengan altivex-pangrango.
2. Klik "add domain" → masukkan `<proj>` → submit.
3. Set IP `35.219.1.25` (sama dengan altivex).
4. Tunggu 1-2 menit sampai propagate (test: `dig <proj>.duckdns.org`).
5. Kalau pakai certbot wajib DNS sudah resolve sebelum issue cert.

## 8. Checklist verifikasi sebelum claim "deployed"

Agent project kedua HARUS jalankan & laporkan output dari semua perintah ini:

```bash
# 1. Container project tidak konflik
docker ps --format "{{.Names}}|{{.Ports}}" | grep <proj>_

# 2. Container ALTIVEX masih hidup
docker ps --format "{{.Names}}|{{.Status}}" | grep altivex_

# 3. Port host yang dipublish project kedua TIDAK overlap dengan ALTIVEX
ss -tlnp | grep -E ':(80|443|1883|8080|<port_baru>)'

# 4. Network terpisah (kecuali Opsi A — broker shared)
docker network ls | grep -E '(altivex|<proj>)'

# 5. nginx valid
sudo nginx -t

# 6. Domain resolve
dig +short <proj>.duckdns.org

# 7. HTTPS endpoint hidup
curl -I https://<proj>.duckdns.org

# 8. ALTIVEX HTTPS masih hidup (regression check)
curl -I https://altivex-pangrango.duckdns.org

# 9. Kalau opsi A — test publish project kedua tidak nyangkut ke ALTIVEX
docker exec altivex_mosquitto mosquitto_sub -u altivex -P "$ALTIVEX_PWD" -t 'altivex/#' -C 1 -W 5
# (publish dari project kedua ke <proj>/test — harus TIDAK muncul di output di atas)
```

Semua harus PASS sebelum claim done.

## 9. Larangan keras (red lines)

1. **JANGAN ubah `~/ALTIVEX/` apa pun**. Repo ALTIVEX adalah read-only
   dari sudut pandang agent project kedua.
2. **JANGAN restart container ALTIVEX** kecuali wajib (mis. tambah user
   MQTT di Opsi A) — dan kalau iya, konfirmasi user dulu.
3. **JANGAN edit `/etc/nginx/sites-enabled/altivex`** — buat file baru
   terpisah.
4. **JANGAN re-issue cert untuk altivex-pangrango.duckdns.org** —
   certbot bisa konflik dengan renew schedule existing.
5. **JANGAN pakai port 80, 443, 1883, 8080 di host** untuk container
   project kedua.
6. **JANGAN commit `.env` ke git**. Pastikan `.gitignore` punya `.env*`
   sejak commit pertama.
7. **JANGAN salin password ALTIVEX**. Generate fresh per project.
8. **JANGAN pakai username MQTT `altivex`** kalau Opsi A — pakai
   `<proj>_user`.

## 10. Roadmap migrasi ke Caddy (future, bukan sekarang)

ALTIVEX sekarang pakai nginx host. Suatu hari nanti ada rencana migrasi
ke Caddy proxy dalam container (lihat `README.md` di folder ini).
**Sampai migrasi itu dieksekusi, ikuti pola nginx host di atas.**
Agent project kedua TIDAK boleh inisiatif install Caddy host atau
container, karena akan rebut port 80/443 dengan nginx existing dan
ALTIVEX akan down.

## 11. Yang perlu agent tanyakan ke user di awal

Sebelum mulai coding, agent project kedua HARUS minta user konfirmasi:

1. **Nama project** kebab-case (untuk prefix container/network/volume)?
2. **Subdomain DuckDNS** yang akan dipakai?
3. **Strategi MQTT**: Opsi A (shared broker) atau Opsi B (broker terpisah)?
4. **Stack backend** (Rust/Node/Python)?
5. **Port internal backend** (bukan 8080 — saran: 8081)?
6. **Apakah perlu publish MQTT ke device eksternal**? (kalau iya dan
   pilih Opsi B, butuh GCP firewall rule baru)

Setelah user konfirmasi, baru lanjut buat compose + nginx + ACL/passwd.

---

**Versi brief**: 1.0
**Tanggal**: 2026-05-18
**Maintainer**: Indra (drathekreator)
**Repo referensi**: github.com/drathekreator/ALTIVEX
