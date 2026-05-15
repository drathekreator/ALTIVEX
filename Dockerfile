FROM rust:latest as builder
WORKDIR /app
# Install system dependencies for serialport crate
RUN apt-get update && apt-get install -y libudev-dev pkg-config
COPY . .
# Kita tambahkan target linux musl untuk memastikan statis apabila diperlukan, tapi debian gnu aman.
RUN cargo build --release

FROM debian:bookworm-slim
WORKDIR /app
RUN apt-get update && apt-get install -y libpq-dev ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/altivex_backend /usr/local/bin/altivex_backend
COPY --from=builder /app/frontend ./frontend

# Kita override environment variables
ENV DATABASE_URL=postgres://altivex:rahasia@postgres:5432/altivex_db
ENV SERIAL_PORT=COM3
# Sebenarnya serial_port tidak dipakai jika berjalan di cloud, backend hanya baca MQTT nantinya

EXPOSE 8080
CMD ["altivex_backend"]
