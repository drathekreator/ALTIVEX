# Multi-Project VM — Reverse Proxy Setup

Arsitektur untuk deploy banyak project (ALTIVEX + project lain) di
satu VM GCP tanpa konflik port dan dengan TLS otomatis.

## Konsep

```
                Internet (port 80, 443, 1883, 8883)
                            │
                ┌───────────▼───────────┐
                │   Caddy (host)         │  ← satu-satunya yang publish ke
                │   - 80/443 (HTTP)      │     internet, di-install di host
                │   - 8883 (MQTTS)       │     (bukan dalam compose)
                └───┬─────────┬─────────┘
                    │         │
        ┌───────────┘         └───────────┐
        │ (HTTP routing)        (TLS termination)
        │                                  │
   ┌────▼────┐  ┌────▼────┐  ┌────▼────┐
   │ ALTIVEX │  │project-2│  │project-N│
   │ network │  │ network │  │ network │   ← tiap project pakai
   │  iso    │  │  iso    │  │  iso    │     internal docker network
   │         │  │         │  │         │     yang terisolasi
   │ backend │  │ backend │  │ backend │
   │ db      │  │ db      │  │ db      │
   │ mqtt    │  │ mqtt    │  │ ...     │
   └─────────┘  └─────────┘  └─────────┘
```

Aturan:

1. **Caddy diinstall di HOST** (bukan dalam compose), supaya bisa
   route ke project compose mana pun via Docker network bridge.
2. **Tiap project compose pakai network yang dinamai eksplisit**,
   plus container dinamai dengan prefix project supaya tidak konflik.
3. **Container project TIDAK publish port ke host** kecuali yang
   memang dibutuhkan dari luar (mis. MQTT 8883 untuk device IoT).
4. **Tiap project punya subdomain sendiri**: `altivex.yourdomain`,
   `project2.yourdomain`, dst. DuckDNS gratis untuk multi-subdomain.
5. **Database + secret per project terisolasi** — nama volume,
   credential, network ber-prefix project.

## Prerequisite (sekali per VM)

```bash
# 1. Install Caddy di host (Ubuntu / Debian)
sudo apt install -y debian-keyring debian-archive-keyring apt-transport-https curl
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' | sudo gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' | sudo tee /etc/apt/sources.list.d/caddy-stable.list
sudo apt update && sudo apt install -y caddy

# 2. Stop nginx kalau sebelumnya pakai nginx (Caddy ambil 80/443)
sudo systemctl stop nginx
sudo systemctl disable nginx

# 3. GCP firewall — buka 80, 443, dan port project lain (8883 utk MQTTS)
# Sudah ada altivex-mqtt (1883). Tambah utk MQTTS:
gcloud compute firewall-rules create altivex-mqtts \
    --allow tcp:8883 \
    --target-tags=mqtt-broker
```

## Tiap project compose pattern

```yaml
# project/docker-compose.yml
networks:
  default:
    name: <project>_internal     # ← unik per project

services:
  postgres:
    container_name: <project>_postgres
    environment:
      POSTGRES_USER: ${POSTGRES_USER}
      ...
    # TIDAK publish 5432 ke host
    volumes:
      - <project>_pgdata:/var/lib/postgresql/data

  backend:
    container_name: <project>_backend
    # TIDAK publish 8080 ke host — Caddy akses lewat
    # `<project>_backend:8080` di network bridge
    expose:
      - "8080"

volumes:
  <project>_pgdata:
```

Pakai prefix `<project>_` untuk **container_name**, **network name**,
**volume name** supaya tidak collision.

## Caddy host config (`/etc/caddy/Caddyfile`)

Edit `/etc/caddy/Caddyfile`:

```caddy
# ALTIVEX
altivex-pangrango.duckdns.org {
    reverse_proxy altivex_backend:8080 {
        header_up X-Forwarded-Host {host}
    }
}

# Project 2 (contoh)
project2.duckdns.org {
    reverse_proxy project2_backend:3000
}

# Project 3 — kalau punya frontend SPA + backend API split
project3.duckdns.org {
    handle /api/* {
        reverse_proxy project3_backend:8080
    }
    handle {
        reverse_proxy project3_frontend:80
    }
}

# MQTTS terminate di Caddy, forward ke broker plaintext internal
# (broker tidak perlu setup TLS sendiri)
:8883 {
    tls /etc/letsencrypt/live/altivex-pangrango.duckdns.org/fullchain.pem \
        /etc/letsencrypt/live/altivex-pangrango.duckdns.org/privkey.pem
    reverse_proxy altivex_mosquitto:1883
}
```

Reload Caddy:

```bash
sudo systemctl reload caddy
```

Caddy auto-issue cert Let's Encrypt untuk semua domain di config.

## Connect Caddy ke Docker network

Caddy host harus bisa ping `altivex_backend:8080`. Caranya:
buat network attached, lalu join Caddy ke network masing-masing project:

```bash
# Setiap kali deploy project baru, tambah Caddy ke network-nya:
docker network connect altivex_internal caddy_host_proxy
```

Tapi karena Caddy install di host (bukan container), pakai `extra_hosts`
atau pakai approach **Caddy dalam container yang join semua project network**:

```yaml
# /opt/caddy-proxy/docker-compose.yml
services:
  caddy:
    image: caddy:2-alpine
    container_name: caddy_proxy
    restart: unless-stopped
    ports:
      - "80:80"
      - "443:443"
      - "443:443/udp"
      - "8883:8883"
    volumes:
      - ./Caddyfile:/etc/caddy/Caddyfile:ro
      - caddy_data:/data
      - caddy_config:/config
    networks:
      - altivex_internal
      - project2_internal
      - project3_internal

networks:
  altivex_internal:
    external: true
  project2_internal:
    external: true
  project3_internal:
    external: true

volumes:
  caddy_data:
  caddy_config:
```

Caddy join semua network, jadi bisa hit `altivex_backend:8080` etc.

## Migrasi ALTIVEX ke pattern ini

1. **Tambah network eksplisit** di `docker-compose.yml` ALTIVEX:

```yaml
networks:
  default:
    name: altivex_internal
```

2. **Hapus port publish dari backend**:

```yaml
backend:
  # ports:
  #   - "8080:8080"   ← hapus
  expose:
    - "8080"
```

3. **Pertahankan port publish hanya di mosquitto** (jika butuh device
   publish dari luar). Atau pindah ke MQTTS via Caddy supaya plaintext
   1883 cukup di internal docker network.

4. **Reload compose**:

```bash
cd ~/ALTIVEX
docker compose down
docker compose up -d
```

5. **Caddy config** otomatis route `altivex-pangrango.duckdns.org`
   ke `altivex_backend:8080`.

## Project baru (template)

```bash
# 1. Buat folder
mkdir -p /opt/projects/project2
cd /opt/projects/project2

# 2. Buat compose
cat > docker-compose.yml <<EOF
networks:
  default:
    name: project2_internal

services:
  backend:
    image: nginx  # contoh
    container_name: project2_backend
    expose:
      - "80"

# Tidak publish port apa-apa
EOF

# 3. Hidupkan
docker compose up -d

# 4. Daftar Caddy
docker network connect project2_internal caddy_proxy

# 5. Tambah block di /etc/caddy/Caddyfile (atau Caddyfile compose)
# project2.yourdomain.com {
#     reverse_proxy project2_backend:80
# }
sudo systemctl reload caddy
# atau kalau Caddy dalam container:
docker compose -f /opt/caddy-proxy/docker-compose.yml exec caddy caddy reload
```

## Untung-nya

- Tidak ada konflik port di host (cuma Caddy yang publish 80/443/8883).
- TLS otomatis untuk semua subdomain.
- Tiap project bisa punya stack berbeda (Rust + Node + Python) — tidak
  saling interfere.
- Hapus project tinggal `docker compose down -v` di folder-nya, network
  + volume terhapus bersih.
- Backup per-project: tinggal backup folder + volume yang ber-prefix.

## Untuk ALTIVEX yang sekarang sudah jalan

Saya sarankan urutan migrasi:

1. **Pertahankan setup sekarang dulu** sampai pipeline MQTT-DB-WS
   benar-benar bekerja (selesaikan masalah saat ini).
2. **Lalu tambah Caddy proxy** di host.
3. **Lalu refactor compose** ALTIVEX (hapus port publish backend, ganti
   nginx jadi Caddy).

Step 2-3 saya bantu kalau Anda sudah selesai dengan masalah MQTT.
