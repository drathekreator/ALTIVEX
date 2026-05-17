# =====================================================================
# ALTIVEX backend — multi-stage Dockerfile
# ---------------------------------------------------------------------
# - Pin Rust toolchain ke versi tertentu untuk build reproducibility.
# - Cache layer untuk cargo deps supaya rebuild incremental cepat.
# - Runtime image slim (debian-bookworm) — hanya bawa biner + frontend.
# =====================================================================

FROM rust:1.90-bookworm AS builder

WORKDIR /app

# Dependencies system untuk crate `serialport` (libudev) dan
# `sqlx-postgres` (libpq dipakai saat runtime, bukan build, tapi
# pkg-config dibutuhkan untuk linker).
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        libudev-dev \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Cache layer untuk dependencies. Copy hanya manifest dulu, build
# dummy main, lalu copy source asli — cara klasik biar `cargo` tidak
# rebuild deps tiap perubahan source.
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src \
    && echo "fn main() { println!(\"dummy\"); }" > src/main.rs \
    && cargo build --release \
    && rm -rf src target/release/altivex_backend target/release/deps/altivex_backend*

# Sekarang copy source sebenarnya + build final.
COPY src ./src
COPY tests ./tests
RUN cargo build --release --bin altivex_backend

# ---------------------------------------------------------------------
# Runtime image
# ---------------------------------------------------------------------
FROM debian:bookworm-slim

WORKDIR /app

# Runtime deps:
# - libpq5: dynamic library yang dibutuhkan sqlx-postgres saat connect.
# - ca-certificates: untuk TLS connect ke broker MQTT remote.
# - libudev1: serialport butuh udev runtime di Linux.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        libpq5 \
        ca-certificates \
        libudev1 \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 1001 altivex \
    && useradd --system --uid 1001 --gid altivex --no-create-home altivex

COPY --from=builder /app/target/release/altivex_backend /usr/local/bin/altivex_backend
COPY frontend ./frontend

# Backend bind ke 0.0.0.0:8080. Reverse proxy (nginx) di-deploy di
# luar container untuk TLS termination.
EXPOSE 8080

# Drop ke user non-root supaya container tidak punya akses root host.
USER altivex:altivex

CMD ["altivex_backend"]
