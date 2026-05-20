# ALTIVEX Demo Branch — Deployment Guide

## Konsep

Satu VM bisa menjalankan **banyak cabang ALTIVEX** (per gunung) secara
paralel. Setiap cabang = stack Docker terpisah dengan:

- Postgres sendiri (database terpisah, data tidak bocor antar cabang)
- Mosquitto sendiri (port MQTT berbeda, credential berbeda)
- Backend container sendiri (share image, beda env)
- GEO.json sendiri (peta geofencing per gunung)
- Login credential sendiri (BASECAMP_USERNAME / BASECAMP_PASSWORD)

## Estimasi Storage

| Komponen | Ukuran |
|----------|--------|
| Docker image backend (shared) | ~150 MB |
| Postgres data (awal) | ~30 MB |
| Mosquitto persistence | ~5 MB |
| GEO.json per gunung | ~50 KB |
| **Total per cabang tambahan** | **~200 MB** |

Dengan disk 50 GB dan prod ~2 GB, kamu bisa punya **20+ cabang** tanpa
masalah. Setelah setahun pemakaian penuh (100k row sensor), satu cabang
naik ~500 MB.

## Quick Start (di VM)

```bash
cd ~/ALTIVEX

# 1. Bootstrap demo instance
bash deployment/demo-branch/bootstrap-demo.sh

# 2. Bawa naik stack demo
docker compose -f deployment/demo-branch/docker-compose.demo.yml up -d --build

# 3. Cek log
docker compose -f deployment/demo-branch/docker-compose.demo.yml logs -f backend-demo

# 4. Update nginx/Caddy untuk route altivex-demo.duckdns.org → port 8081
#    (lihat section Reverse Proxy di bawah)
```

## Firewall GCP (sekali)

Buka port 1885 untuk MQTT demo:

```bash
gcloud compute firewall-rules create altivex-mqtt-demo \
    --allow tcp:1885 \
    --target-tags=mqtt-broker \
    --description="MQTT port untuk ALTIVEX demo branch"
```

---

## Reverse Proxy (nginx yang sudah ada)

Tambahkan server block baru di nginx config VM:

```nginx
server {
    listen 443 ssl http2;
    server_name altivex-demo.duckdns.org;

    ssl_certificate     /etc/letsencrypt/live/altivex-demo.duckdns.org/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/altivex-demo.duckdns.org/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:8081;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}

server {
    listen 80;
    server_name altivex-demo.duckdns.org;
    return 301 https://$host$request_uri;
}
```

Lalu:
```bash
sudo certbot certonly --nginx -d altivex-demo.duckdns.org
sudo nginx -t && sudo systemctl reload nginx
```

## Ganti Peta Geofencing

Taruh file GeoJSON jalur gunung yang berbeda di:
```
deployment/demo-branch/frontend-override/GEO.json
```

File ini akan di-mount ke container demo, menimpa GEO.json default.
Format sama persis dengan `frontend/GEO.json` yang sudah ada (FeatureCollection
dengan LineString untuk jalur + Point untuk waypoint).

## Menambah Cabang Lain (misal Gunung Semeru)

1. Copy folder `deployment/demo-branch/` → `deployment/semeru-branch/`
2. Edit `docker-compose.demo.yml`:
   - Ganti semua `demo` → `semeru`
   - Ganti port `8081` → `8082`, `1884` → `1885`
   - Ganti volume name `pgdata_demo` → `pgdata_semeru`
3. Jalankan `bootstrap-demo.sh` (akan generate `.env.demo` baru)
4. Tambah nginx server block untuk domain `altivex-semeru.duckdns.org` → port 8082

## Hapus Demo Instance

```bash
# Stop + hapus container + volume (data hilang permanen)
docker compose -f deployment/demo-branch/docker-compose.demo.yml down -v

# Hapus env file
rm -f deployment/demo-branch/.env.demo
```

## MQTT untuk ESP32 Demo

ESP32 yang dipakai demo harus publish ke:
- Host: `altivex-demo.duckdns.org` (atau IP VM langsung)
- Port: `1885` (bukan 1883!)
- Username/Password: lihat output `bootstrap-demo.sh`
- Topic: `altivex/sensor/data` (sama)
