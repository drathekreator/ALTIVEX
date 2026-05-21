# Trail Digitizer (Pangrango)

Tool untuk **manual digitize** jalur pendakian Gunung Gede-Pangrango ke
GeoJSON polyline yang ngikutin jalur asli (bukan vektor lurus).

## Kenapa manual?

PDF peta jalur pendakian itu **raster image** (illustrated map), bukan
vector data. OSRM publik gak punya profile `foot/hike` yang kenal jalur
gunung. Jadi cara paling akurat = digitize manual dengan referensi peta
PDF + Leaflet.

## Cara pakai

### 1. Buka tool

```
http://localhost:8080/tools/digitize.html
```

Atau langsung buka file lokal (drag `frontend/tools/digitize.html` ke
browser). Tile map dari OSM, gak butuh koneksi ke backend.

### 2. Load GEO.json existing

Klik **"Choose File"** di section #1, pilih
`frontend/GEO.json`. Tool akan render semua waypoint Pos/Shelter sebagai
referensi. Kalau sudah ada LineString lama, dia di-load ke editor supaya
bisa di-extend (gak harus mulai dari nol).

### 3. Buka PDF di tab kedua (referensi)

Klik **"📄 Buka PDF di tab baru"**, pilih
`peta-jalur-pendakian-gunung-gede-pangrango.pdf`. PDF terbuka di tab
terpisah — split window di monitor: kiri Leaflet, kanan PDF.

### 4. Digitize per jalur

1. Klik tombol jalur aktif (mis. **"Cibodas"**)
2. Klik di peta titik pertama (trailhead Cibodas)
3. Lihat PDF: jalur Cibodas membentang ke mana? Ikuti dengan klik per
   titik di Leaflet, mengikuti kontur di PDF
4. Setiap waypoint Pos/Shelter yang sudah ada (dari GEO.json) akan
   muncul sebagai referensi titik
5. Klik titik existing untuk hapus (kalau ada salah klik)
6. Klik **"↶ Undo"** untuk hapus titik terakhir
7. Pindah ke jalur berikutnya (Gunung Putri, Selabintana), ulangi

**Density saran:** ~30-50 titik per jalur cukup untuk visualisasi yang
smooth tanpa polyline kelihatan zigzag patah-patah.

### 5. Export

Klik **"⬇ Download GEO.json"**. File akan ter-download ke folder
Downloads kamu. Replace `frontend/GEO.json` dengan file ini:

```powershell
Copy-Item -Force "$env:USERPROFILE\Downloads\GEO.json" `
    "C:\Users\USER\Documents\ALTIVEX\altivex_backend\frontend\GEO.json"
```

## Verifikasi hasil

1. Refresh dashboard prod (`https://altivex-pangrango.duckdns.org/`)
2. Polyline jalur Cibodas/Gunung Putri/Selabintana akan ngikutin jalur
   yang kamu klik, bukan garis lurus titik-ke-titik
3. Geofence buffer (50m) otomatis ngikutin lekukan polyline

## Notes

- LineString lama di GEO.json akan **di-replace** total saat export
  (waypoint Point tetap dipertahankan)
- Save WIP: kalau belum selesai, export, save ke disk, lanjut nanti
  dengan reload file itu
- Browser cache: kalau perubahan gak muncul di prod, hard reload
  (Ctrl+Shift+R) dan/atau restart container backend
