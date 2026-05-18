# ALTIVEX Icon Library

Source-of-truth design ikon untuk dashboard ALTIVEX.

## Konvensi

Semua ikon mengikuti aturan:

- **viewBox**: `0 0 24 24` (standard icon size)
- **stroke**: `2.5` (tebal khas neobrutalism)
- **stroke-linecap**: `round`
- **stroke-linejoin**: `round`
- **fill**: `none` di element default; titik solid pakai `fill="currentColor"`
- **Color**: `currentColor` (auto adapt theme via CSS `color`)

## Cara pakai di code

Ikon sudah inline di `frontend/icons.js` sebagai `ICON_PATHS`. Pakai:

```js
// HTML string (innerHTML)
el.innerHTML = `<button>${ICON('alert', 18)} ALERT</button>`;

// DOM Node
const node = iconNode('mountain', 32);
header.appendChild(node);
```

## Cara edit / tambah ikon baru

1. Edit di Figma sesuai konvensi 24×24, stroke 2.5px.
2. Export ke SVG → simpan ke folder ini (mis. `mountain.svg`).
3. Copy bagian `<path>` / `<circle>` / `<rect>` ke `ICON_PATHS` di
   `frontend/icons.js`. Hapus atribut `stroke`, `stroke-width`,
   `fill` karena sudah di-handle wrapper.
4. Test di dashboard pakai `ICON('namaBaru', 18)`.

## Daftar ikon (sinkron dengan `icons.js`)

| Nama          | Pemakaian                                            |
|---------------|------------------------------------------------------|
| mountain      | Logo brand di header                                 |
| moon, sun     | Theme toggle                                         |
| unlock        | Tombol login                                         |
| logout        | Tombol logout di header                              |
| user, users   | Tab pendaki, modal registrasi                        |
| bell, bellAlert | Toast notif, alert banner                          |
| warning       | In-app alert banner, badge "Keluar koridor"          |
| check, checkSimple | Tombol "Selesai", toast success                 |
| cross         | Tombol "Batal" (close)                               |
| info          | Toast info                                           |
| device        | Card standby device, dropdown ID Perangkat           |
| broadcast     | Status MQTT, tab "Peta Live"                         |
| satellite     | Marker device di peta                                |
| download      | Export CSV                                           |
| plus          | Daftarkan pendaki baru                               |
| edit          | Edit data pendaki                                    |
| trash         | Hapus pendaki                                        |
| map           | View polyline detail pendaki                         |
| search        | Filter cari nama                                     |
| refresh       | "Memuat data live..."                                |
| clock         | Waktu naik (timestamp)                               |
| flag          | Waypoint Trailhead                                   |
| gate          | Waypoint Gate (gerbang)                              |
| home          | Waypoint Pos                                         |
| tent          | Waypoint Camp                                        |
| summit        | Waypoint Summit (puncak)                             |
| junction      | Waypoint Junction (cabang)                           |
| pin           | Waypoint default / generic                           |
| water         | Waypoint sumber air                                  |
| circleDot     | Marker mulai polyline (start)                        |
| chartLine     | Statistik jarak tempuh                               |
| arrowRight    | Tombol next / continue                               |
| arrowLeft     | Tombol back                                          |
| eye           | Lihat detail pendaki                                 |
| shield        | Status security / auth aktif                        |

## Color & dark mode

Ikon di-render dengan `stroke="currentColor"`. CSS yang menentukan warna:

```css
.icon                                   { color: var(--ink); }
body.dark-mode .icon                    { color: var(--ink); }   /* token sudah ke-flip */
.alert-card .icon                       { color: var(--white); }  /* override per konteks */
.neo-btn-red .icon                      { color: white; }
```

Kalau butuh ikon dengan warna fixed (mis. branding), pakai inline
`style="color: var(--primary)"` pada wrapper-nya, jangan rombak SVG.
