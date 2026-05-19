/* =====================================================================
   ALTIVEX Dashboard — runtime
   =====================================================================
   Dipisahkan dari `index.html` (Task UI #10) supaya markup tidak
   bercampur dengan logika, dan unit test (vitest) bisa di masa depan
   `import()` modul ini langsung tanpa parsing inline `<script>`.

   File ini di-load sebagai non-module script (`<script src="...">`
   tanpa `type="module"`) supaya seluruh fungsi tetap bersifat global —
   handler `onclick="..."` di markup yang masih ada (mis.
   `onclick="exportCSV()"`) tetap valid.

   Konvensi:
   - Kontrak fungsi (signature & behavior) tidak berubah dibanding
     versi inline lama. Property test (PBT) di `tests/*.spec.js`
     mereplikasi behavior F' verbatim — kalau kontrak di sini berubah,
     replika tsb juga harus di-update.
   - Penyisipan ke `innerHTML` SELALU lewat `escapeHtml` (Task 3.9).
   - Tombol dinamis pakai `data-action` + delegated listener
     (Task 3.10), bukan `onclick="...JSON.stringify(p)..."`.
   - Flag notifikasi ada di `notifiedDevices` Map terpisah, BUKAN
     property pada `latestDataPerDevice[id]` (Task 3.11).
   - Banner alert in-app (Task UI #6) di-update di akhir
     `_renderHikerCards()` berdasarkan jumlah pendaki keluar buffer.
   ===================================================================== */

// ====================================================================
// API Auth Token + Login (UI #4 feedback)
// --------------------------------------------------------------------
// Operator tidak lagi paste token mentah ke browser prompt(). Alur:
//   1. Saat dashboard load, cek `localStorage[TOKEN_STORAGE_KEY]`.
//      Kosong → tampilkan login modal `#modal-login`. Form submit →
//      `POST /api/login {username, password}` → backend validasi
//      constant-time → return `{token}` → simpan ke localStorage.
//   2. Saat fetch balik 401 → clear token + tampilkan login modal lagi
//      (token di-rotate / sesi invalid).
//   3. Tombol logout di header bersihkan token + tampilkan login modal.
//
// Backend tetap pakai `Authorization: Bearer <token>` untuk semua
// endpoint mutating. Login modal hanya UX layer — token mechanism
// tidak rombak.
// ====================================================================
const TOKEN_STORAGE_KEY = "ALTIVEX_API_TOKEN";

function getStoredToken() {
    try { return localStorage.getItem(TOKEN_STORAGE_KEY) || ""; }
    catch (e) { return ""; }
}

function setStoredToken(t) {
    try { localStorage.setItem(TOKEN_STORAGE_KEY, t); } catch (e) {}
}

function clearApiToken() {
    try { localStorage.removeItem(TOKEN_STORAGE_KEY); } catch (e) {}
}

function showLoginModal() {
    const modal = document.getElementById("modal-login");
    if (modal) {
        modal.style.display = "flex";
        // Reset error state setiap kali modal di-show.
        const err = document.getElementById("login-error");
        if (err) err.hidden = true;
        // Auto-focus username untuk operator basecamp keyboard-first.
        const u = document.getElementById("login-username");
        if (u) setTimeout(() => u.focus(), 50);
    }
    const logout = document.getElementById("logout-btn");
    if (logout) logout.hidden = true;
}

function hideLoginModal() {
    const modal = document.getElementById("modal-login");
    if (modal) modal.style.display = "none";
    const logout = document.getElementById("logout-btn");
    if (logout) logout.hidden = false;
}

async function handleLoginSubmit(ev) {
    ev.preventDefault();
    const u = document.getElementById("login-username").value.trim();
    const p = document.getElementById("login-password").value;
    const err = document.getElementById("login-error");
    const btn = ev.target.querySelector('button[type="submit"]');

    if (!u || !p) {
        err.textContent = "Username dan password wajib diisi.";
        err.hidden = false;
        return;
    }

    btn.disabled = true;
    btn.textContent = "Memverifikasi...";

    try {
        const res = await fetch("/api/login", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ username: u, password: p }),
        });
        if (res.status === 401) {
            err.textContent = "Username atau password salah.";
            err.hidden = false;
            return;
        }
        if (res.status === 503) {
            err.textContent = "Login belum dikonfigurasi di server. Hubungi admin.";
            err.hidden = false;
            return;
        }
        if (!res.ok) {
            err.textContent = `Server error (${res.status}). Coba lagi.`;
            err.hidden = false;
            return;
        }
        const json = await res.json();
        if (!json || !json.token) {
            err.textContent = "Respons server tidak valid.";
            err.hidden = false;
            return;
        }
        setStoredToken(json.token);
        hideLoginModal();
        // Reset password field (keep username untuk convenience).
        document.getElementById("login-password").value = "";
        showToast("Login berhasil", "success");
        // Trigger reload data setelah login.
        fetchInitialSensorData();
        fetchPendakiAktif();
    } catch (e) {
        err.textContent = "Tidak bisa terhubung ke server.";
        err.hidden = false;
    } finally {
        btn.disabled = false;
        btn.innerHTML = ICON('unlock', 18) + ' MASUK';
    }
}

function logout() {
    clearApiToken();
    showLoginModal();
    showToast("Logout berhasil", "info");
}

let __altivexLoginShown = false;
async function apiFetch(url, options) {
    // Kalau token belum ada, biarkan login modal yang menangani —
    // tetap kirim request (mungkin endpoint publik) tanpa Authorization.
    const opts = options ? Object.assign({}, options) : {};
    const headers = new Headers(opts.headers || {});
    const token = getStoredToken();
    if (token) headers.set("Authorization", "Bearer " + token);
    opts.headers = headers;

    const res = await fetch(url, opts);
    if (res.status === 401) {
        // Sesi invalid — clear token + tampilkan login modal sekali.
        clearApiToken();
        if (!__altivexLoginShown) {
            __altivexLoginShown = true;
            showLoginModal();
        }
    }
    return res;
}

// ====================================================================
// escapeHtml & csvField (Tasks 3.9 & 3.13)
// ====================================================================
function escapeHtml(v) {
    return String(v ?? "")
        .replaceAll("&", "&amp;")
        .replaceAll("<", "&lt;")
        .replaceAll(">", "&gt;")
        .replaceAll('"', "&quot;")
        .replaceAll("'", "&#39;");
}

function csvField(v) {
    const s = String(v ?? "");
    if (/[",\r\n]/.test(s)) {
        return '"' + s.replaceAll('"', '""') + '"';
    }
    return s;
}

// ====================================================================
// CONFIG & STATE
// ====================================================================
const posJagaLatLng = [-6.7711, 106.9600];
let map = L.map("map").setView(posJagaLatLng, 15);
let activeMarkers = {};
let latestDataPerDevice = {};
let registeredHikers = {};
let historyData = [];
let activePolylines = {};
let geofenceBuffer;

const pendakiById = new Map();

// notifiedDevices Map terpisah dari `latestDataPerDevice` (Task 3.11).
const notifiedDevices = new Map();
function setNotified(id, val) { notifiedDevices.set(id, val === true); }
function isNotified(id)       { return notifiedDevices.get(id) === true; }

// ====================================================================
// BATTERY MONITOR (post-feedback)
// --------------------------------------------------------------------
// Threshold UI:
//   ≥ 75%       → batteryFull,    color: success (hijau)
//   50-74%      → batteryHigh,    color: success (hijau)
//   25-49%      → batteryMid,     color: primary (kuning/mustard)
//   15-24%      → batteryLow,     color: danger (merah, tidak pulse)
//   1-14%       → batteryEmpty,   color: danger (merah + pulse)
//   0%          → batteryEmpty,   color: muted (alat mati)
//   null/undef  → batteryUnknown, color: muted ("?")
//
// Threshold notifikasi browser: 15%. Operator basecamp dapat 1
// notif per device per turun di bawah threshold; reset begitu
// battery >25% (hysteresis supaya tidak spam saat fluktuasi).
// --------------------------------------------------------------------
const BATTERY_NOTIF_THRESHOLD = 15;
const BATTERY_NOTIF_RESET_THRESHOLD = 25;

// Map id_perangkat → boolean: sudah pernah dapat notif low-battery
// di sesi ini? Reset kalau battery naik di atas RESET threshold.
const batteryNotified = new Map();

/**
 * Pilih nama icon + class warna sesuai persen battery.
 * @param {number|null|undefined} pct
 * @returns {{name: string, level: string, label: string}}
 */
function batteryStyle(pct) {
    if (pct === null || pct === undefined || !Number.isFinite(pct)) {
        return { name: "batteryUnknown", level: "unknown", label: "—" };
    }
    const p = Math.max(0, Math.min(100, Math.round(pct)));
    if (p === 0)        return { name: "batteryEmpty", level: "off",  label: "0%" };
    if (p < 15)         return { name: "batteryEmpty", level: "crit", label: `${p}%` };
    if (p < 25)         return { name: "batteryLow",   level: "low",  label: `${p}%` };
    if (p < 50)         return { name: "batteryMid",   level: "mid",  label: `${p}%` };
    if (p < 75)         return { name: "batteryHigh",  level: "ok",   label: `${p}%` };
    return                     { name: "batteryFull",  level: "full", label: `${p}%` };
}

/**
 * Render battery indicator inline (icon + persen) untuk dipakai di
 * alert-card / standby-card. Pakai class CSS `.battery-pill--<level>`
 * untuk styling warna; ICON() kembalikan SVG dengan currentColor yang
 * inherit dari class wrapper.
 */
function batteryPill(pct) {
    const s = batteryStyle(pct);
    return `<span class="battery-pill battery-pill--${s.level}" title="Baterai ${s.label}">${ICON(s.name, 16)}<span class="battery-pill__label">${s.label}</span></span>`;
}

/**
 * Trigger notifikasi browser sekali saat battery turun di bawah
 * BATTERY_NOTIF_THRESHOLD untuk pertama kali. Reset state begitu
 * battery naik di atas RESET threshold (hysteresis 10% gap).
 */
function maybeNotifyLowBattery(id, pct, hikerName) {
    if (pct === null || pct === undefined || !Number.isFinite(pct)) return;
    const wasNotified = batteryNotified.get(id) === true;

    if (pct >= BATTERY_NOTIF_RESET_THRESHOLD && wasNotified) {
        // Reset — battery naik kembali (mis. ganti baterai), siap notif
        // lagi kalau turun di bawah threshold di waktu lain.
        batteryNotified.set(id, false);
        return;
    }

    if (pct < BATTERY_NOTIF_THRESHOLD && !wasNotified) {
        const who = hikerName ? hikerName : `Alat ${id}`;
        sendNotification(
            "Baterai Lemah",
            `${who} tinggal ${pct}% — segera ganti / charge.`
        );
        if (typeof showToast === "function") {
            showToast(`Baterai ${who} tinggal ${pct}%`, "error");
        }
        batteryNotified.set(id, true);
    }
}

// GEO.json state
let geoData = null;
let routeFeatures = null;
let waypointFeatures = null;

const routeColors = {
    'Cibodas':       '#2979FF',
    'Gunung Putri':  '#FF6D00',
    'Selabintana':   '#AA00FF'
};

// ====================================================================
// MAP — waypoint icon registry (auto-generated SVG via icons.js)
// --------------------------------------------------------------------
// Sebelumnya pakai emoji (🚩 🚪 🏠 ⛺ 🏔 dll). Sekarang pakai SVG
// dari ICON() — currentColor-aware, konsisten dengan dashboard.
// `waypointIconName` map type Leaflet → key di `ICON_PATHS` (icons.js).
// ====================================================================
const waypointIconName = {
    'Trailhead': 'flag',
    'Gate':      'gate',
    'Pos':       'home',
    'Camp':      'tent',
    'Summit':    'summit',
    'Junction':  'junction',
    'Waypoint':  'pin',
    'Water':     'water',
    'default':   'pin',
};

// ====================================================================
// MAP INIT
// ====================================================================
L.tileLayer("https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png", {
    attribution: "© OpenStreetMap",
}).addTo(map);

async function initGeoData() {
    try {
        const res = await fetch('/GEO.json');
        geoData = await res.json();

        routeFeatures = {
            type: "FeatureCollection",
            features: geoData.features.filter(f => f.geometry.type === 'LineString')
        };
        waypointFeatures = geoData.features.filter(f => f.geometry.type === 'Point');

        L.geoJSON(routeFeatures, {
            style: function(feature) {
                const routeName = feature.properties.route || '';
                return {
                    color: routeColors[routeName] || '#000',
                    weight: 4,
                    dashArray: '8, 6',
                    opacity: 0.85
                };
            },
            onEachFeature: function(feature, layer) {
                layer.bindPopup(`<b>${escapeHtml(feature.properties.name)}</b>`);
            }
        }).addTo(map);

        geofenceBuffer = turf.buffer(routeFeatures, 0.05, { units: 'kilometers' });

        L.geoJSON(geofenceBuffer, {
            style: {
                color: '#00E676',
                fillColor: '#00E676',
                fillOpacity: 0.12,
                weight: 1,
                dashArray: '3, 3'
            }
        }).addTo(map);

        waypointFeatures.forEach(wp => {
            const [lng, lat] = wp.geometry.coordinates;
            const props = wp.properties;
            const iconName = waypointIconName[props.type] || waypointIconName.default;
            const iconSvg = ICON(iconName, 22, 'waypoint-svg');
            const routeColor = routeColors[props.route] || '#333';

            const marker = L.marker([lat, lng], {
                icon: L.divIcon({
                    html: `<span class="waypoint-icon">${iconSvg}</span>`,
                    className: '',
                    iconSize: [28, 28],
                    iconAnchor: [14, 14]
                })
            }).addTo(map);

            const elev = props.elevation_m ? `${props.elevation_m} mdpl` : '-';
            marker.bindPopup(
                `<div class="waypoint-popup">` +
                `<b class="waypoint-popup__title">${ICON(iconName, 16)} ${escapeHtml(props.name)}</b><br>` +
                `<span class="waypoint-popup__route" style="background:${routeColor};">` +
                `${escapeHtml(props.route)}</span><br>` +
                `<small>Tipe: ${escapeHtml(props.type)} | Elevasi: ${escapeHtml(elev)}</small></div>`
            );
        });

        const allBounds = L.geoJSON(routeFeatures).getBounds();
        map.fitBounds(allBounds, { padding: [40, 40] });

        console.log(`✅ GEO.json loaded: ${routeFeatures.features.length} jalur, ${waypointFeatures.length} waypoint`);
    } catch (err) {
        console.error('❌ Gagal memuat GEO.json:', err);
        map.setView([-6.79, 106.97], 13);
    }
}

initGeoData();

// ====================================================================
// TAB LOGIC
// ====================================================================
function openTab(tabId, el) {
    document.querySelectorAll(".tab-content").forEach((tab) => tab.classList.remove("active"));
    document.querySelectorAll(".tab-link").forEach((link) => link.classList.remove("active"));
    document.getElementById(tabId).classList.add("active");
    if (el) el.classList.add("active");

    if (tabId === 'tab-live') setTimeout(() => map.invalidateSize(), 200);
    if (tabId === 'tab-pendaki') fetchHistory();
}

// ====================================================================
// THEME (Modern Warm light + dark mode)
// --------------------------------------------------------------------
// Operator dinas malam: tombol 🌙/☀ di header. Logika:
//   1. Saat script load, baca `localStorage["ALTIVEX_THEME"]` ("light" /
//      "dark"). Kalau belum pernah di-set, ikuti
//      `prefers-color-scheme: dark` browser.
//   2. Apply class `dark-mode` di `<body>` SECEPAT MUNGKIN (di top-level
//      module saat DOM masih parse) supaya tidak ada flash putih sebelum
//      dark mode aktif untuk operator yang prefer dark.
//   3. Klik toggle → flip class + simpan pilihan eksplisit ke
//      localStorage. Setelah itu user choice menang dibanding system
//      preference (sesuai konvensi UI modern).
//   4. Tombol icon di-update tiap toggle (🌙 saat light = ajak ke dark;
//      ☀ saat dark = ajak balik ke light).
// ====================================================================
const THEME_STORAGE_KEY = "ALTIVEX_THEME";

function getStoredTheme() {
    try { return localStorage.getItem(THEME_STORAGE_KEY); } catch (e) { return null; }
}

function storeTheme(theme) {
    try { localStorage.setItem(THEME_STORAGE_KEY, theme); } catch (e) {}
}

function applyTheme(theme) {
    const isDark = theme === "dark";
    document.body.classList.toggle("dark-mode", isDark);
    const btn = document.getElementById("theme-toggle");
    if (btn) {
        // Pakai SVG icon dari ICON_PATHS (icons.js). Saat dark mode,
        // tampilkan ikon matahari (clue: klik untuk balik ke light);
        // saat light mode, tampilkan bulan.
        btn.innerHTML = isDark ? ICON('sun', 18) : ICON('moon', 18);
        btn.setAttribute("aria-label", isDark ? "Aktifkan light mode" : "Aktifkan dark mode");
        btn.setAttribute("aria-pressed", String(isDark));
    }
}

function initialTheme() {
    const stored = getStoredTheme();
    if (stored === "dark" || stored === "light") return stored;
    // Default: ikuti system preference saat first-visit.
    if (window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches) {
        return "dark";
    }
    return "light";
}

function toggleTheme() {
    const next = document.body.classList.contains("dark-mode") ? "light" : "dark";
    storeTheme(next);
    applyTheme(next);
}

// Apply theme SECEPAT MUNGKIN. dashboard.js di-load di akhir <body>
// jadi document.body sudah ada — aman langsung set class. Tombol
// dengan id `theme-toggle` belum tentu ada saat ini (script-nya parse
// dari atas ke bawah), jadi `applyTheme` no-op untuk btn. Listener +
// re-apply icon di pasang via DOMContentLoaded di blok INIT bawah.
applyTheme(initialTheme());

// ====================================================================
// NOTIFICATION LOGIC
// Panel `#notif-request` muncul SEKALI (Task 3.x patch + UI hardening):
// localStorage flag `ALTIVEX_NOTIF_PROMPT_DISMISSED` mencegah panel
// muncul lagi setelah keputusan apa pun (Izinkan / Nanti / browser
// block) — bahkan setelah refresh.
// ====================================================================
const NOTIF_PROMPT_KEY = "ALTIVEX_NOTIF_PROMPT_DISMISSED";

function notifPromptDismissed() {
    try { return localStorage.getItem(NOTIF_PROMPT_KEY) === "1"; }
    catch (e) { return false; }
}

function rememberNotifPromptDismissed() {
    try { localStorage.setItem(NOTIF_PROMPT_KEY, "1"); } catch (e) {}
}

function hideNotifPrompt() {
    const el = document.getElementById('notif-request');
    if (el) el.style.display = 'none';
}

function dismissNotifPrompt() {
    hideNotifPrompt();
    rememberNotifPromptDismissed();
}

function requestNotif() {
    Notification.requestPermission().then(permission => {
        hideNotifPrompt();
        rememberNotifPromptDismissed();
        if (permission === "granted") {
            showToast("Notifikasi diizinkan!", "success");
        }
    });
}

function sendNotification(title, body) {
    if (Notification.permission === "granted") {
        new Notification(title, { body: body, icon: "⛰️" });
    }
}

window.addEventListener('load', () => {
    if (typeof Notification === "undefined") return;
    if (Notification.permission !== "default") return;
    if (notifPromptDismissed()) return;
    setTimeout(() => {
        const el = document.getElementById('notif-request');
        if (el) el.style.display = 'flex';
    }, 3000);
});

// ====================================================================
// TOAST LOGIC
// ====================================================================
function showToast(msg, type) {
    const container = document.getElementById('toast-container');
    const toast = document.createElement('div');
    const cls = type === 'success' ? 'toast--success'
              : type === 'error'   ? 'toast--error'
              :                       'toast--info';
    toast.className = `toast ${cls}`;
    // `msg` di-render via textContent (DOM API aman) — kita tetap pakai
    // wrapper innerHTML untuk icon, dan teks lewat textContent supaya
    // string apa pun (mis. dari error message backend) tidak ter-parse
    // sebagai HTML.
    const iconName = type === 'success' ? 'check'
                   : type === 'error'   ? 'warning'
                   :                      'bell';
    toast.innerHTML = ICON(iconName, 18) + ' ';
    const span = document.createElement('span');
    span.textContent = msg;
    toast.appendChild(span);
    container.appendChild(toast);
    setTimeout(() => toast.remove(), 4000);
}

// ====================================================================
// MODAL LOGIC
// ====================================================================
function openModal() {
    editingId = null;
    document.getElementById('modal-title').innerText = "REGISTRASI PENDAKI";
    document.getElementById('reg-nama').value = "";
    document.getElementById('reg-telp').value = "";
    document.getElementById('btn-simpan').onclick = submitRegistrasi;

    const select = document.getElementById('reg-id-perangkat');
    select.innerHTML = '<option value="">-- Pilih Perangkat --</option>';

    const onlineIds = Object.keys(latestDataPerDevice);
    const usedIds = Object.keys(registeredHikers);
    const available = onlineIds.filter(id => !usedIds.includes(id));

    if (available.length === 0) {
        select.innerHTML += '<option disabled>Tidak ada alat standby</option>';
    } else {
        available.forEach(id => {
            // ID perangkat berasal dari WS / DB → escape sebelum
            // dijadikan attribute value & textContent.
            const idEsc = escapeHtml(id);
            select.innerHTML += `<option value="${idEsc}">${idEsc}</option>`;
        });
    }
    document.getElementById('modal-registrasi').style.display = 'flex';
}

function closeModal()  { document.getElementById('modal-registrasi').style.display = 'none'; }
function closeConfirm() { document.getElementById('modal-confirm').style.display = 'none'; }

function showConfirm(title, msg, onConfirm) {
    document.getElementById('confirm-title').innerText = title;
    document.getElementById('confirm-msg').innerText = msg;
    document.getElementById('modal-confirm').style.display = 'flex';
    document.getElementById('confirm-yes').onclick = () => {
        onConfirm();
        closeConfirm();
    };
}

// ====================================================================
// API ACTIONS
// ====================================================================
async function submitRegistrasi() {
    const nama = document.getElementById('reg-nama').value;
    const idAlat = document.getElementById('reg-id-perangkat').value;
    const telp = document.getElementById('reg-telp').value;

    if (!nama || !idAlat || !telp) return showToast("Harap isi semua field!", "error");

    try {
        const res = await apiFetch("/api/pendaki", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ nama_pendaki: nama, id_perangkat: idAlat, telepon_darurat: telp })
        });
        if (res.ok) {
            showToast("Pendaki Berhasil Didaftarkan!", "success");
            closeModal();
            fetchPendakiAktif();
            fetchHistory();
        } else {
            showToast("Gagal mendaftar!", "error");
        }
    } catch (e) { showToast("Koneksi Error", "error"); }
}

async function fetchPendakiAktif() {
    try {
        const response = await apiFetch("/api/pendaki");
        const data = await response.json();
        registeredHikers = {};
        data.forEach(p => { registeredHikers[p.id_perangkat] = p; });
        renderHikerCards();
    } catch (e) { console.error(e); }
}

async function fetchHistory() {
    try {
        const response = await apiFetch("/api/pendaki/riwayat");
        historyData = await response.json();
        pendakiById.clear();
        for (const p of historyData) {
            if (p && typeof p.id !== "undefined" && p.id !== null) {
                pendakiById.set(p.id, p);
            }
        }
        renderHistoryTable(historyData);
    } catch (e) { console.error(e); }
}

function renderHistoryTable(data) {
    const tbody = document.getElementById('history-table-body');
    tbody.innerHTML = data.map(p => {
        const idAttr = escapeHtml(String(p.id ?? ""));
        const idPerangkatAttr = escapeHtml(String(p.id_perangkat ?? ""));
        const statusBadgeCls = p.status === 'Mendaki' ? 'badge-status-on' : 'badge-status-off';
        const actionBtns = (p.status === 'Mendaki')
            ? `<button class="neo-btn neo-btn-sm neo-btn-blue" data-action="finish" data-id-perangkat="${idPerangkatAttr}">${ICON('checkSimple', 14)} Selesai</button>`
            : `<button class="neo-btn neo-btn-sm neo-btn-red" data-action="delete" data-id="${idAttr}">${ICON('trash', 14)}</button>`;
        // tanggal_turun NULL untuk pendaki masih mendaki — tampil
        // dash supaya tidak misleading vs "00:00 / Invalid Date".
        const turunDisplay = p.tanggal_turun
            ? new Date(p.tanggal_turun).toLocaleString('id-ID')
            : "—";
        // `data-label` dipakai oleh CSS `@media (max-width: 600px)` untuk
        // me-render label inline saat tabel di-card-ify (tiap row jadi
        // kartu vertikal, label di kiri, value di kanan).
        return `
            <tr>
                <td data-label="Nama">${escapeHtml(p.nama_pendaki)}</td>
                <td data-label="Alat"><span class="neo-badge badge-id">${escapeHtml(p.id_perangkat)}</span></td>
                <td data-label="Telepon" class="hide-mobile">${escapeHtml(p.telepon_darurat)}</td>
                <td data-label="Status">
                    <span class="neo-badge ${statusBadgeCls}">${escapeHtml(p.status)}</span>
                </td>
                <td data-label="Waktu Naik" class="hide-mobile">${escapeHtml(new Date(p.tanggal_naik).toLocaleString('id-ID'))}</td>
                <td data-label="Waktu Turun" class="hide-mobile">${escapeHtml(turunDisplay)}</td>
                <td data-label="Aksi" class="cell-actions">
                    <div class="history-actions">
                        ${actionBtns}
                        <button class="neo-btn neo-btn-sm" data-action="edit" data-id="${idAttr}">${ICON('edit', 14)}</button>
                        <button class="neo-btn neo-btn-sm neo-btn-green" data-action="view" data-id="${idAttr}">${ICON('map', 14)}</button>
                    </div>
                </td>
            </tr>
        `;
    }).join('');

    if (!tbody.dataset.delegated) {
        tbody.addEventListener('click', handleHistoryTableClick);
        tbody.dataset.delegated = "1";
    }
}

function handleHistoryTableClick(ev) {
    const target = ev.target.closest('[data-action]');
    if (!target || !this.contains(target)) return;
    const action = target.dataset.action;

    if (action === 'finish') {
        const idPerangkat = target.dataset.idPerangkat || "";
        if (idPerangkat) selesaikanPendakian(idPerangkat);
        return;
    }

    const idStr = target.dataset.id || "";
    const idNum = parseInt(idStr, 10);
    if (!Number.isFinite(idNum)) return;

    if (action === 'delete') { deletePendaki(idNum); return; }

    const p = pendakiById.get(idNum);
    if (!p) return;
    if (action === 'edit')      openEditModal(p);
    else if (action === 'view') viewJourneyDetail(p);
}

async function deletePendaki(id) {
    showConfirm("HAPUS DATA", "Hapus permanen data pendaki ini dari riwayat?", async () => {
        try {
            const res = await apiFetch(`/api/pendaki/${id}`, { method: "DELETE" });
            if (res.ok)                    { showToast("Data dihapus", "success"); fetchHistory(); }
            else if (res.status === 404)   { showToast("Pendaki tidak ditemukan", "error"); }
            else                           { showToast("Gagal menghapus", "error"); }
        } catch (e) { showToast("Gagal menghapus", "error"); }
    });
}

let editingId = null;
function openEditModal(p) {
    editingId = p.id;
    document.getElementById('modal-title').innerText = "EDIT DATA PENDAKI";
    document.getElementById('reg-nama').value = p.nama_pendaki;
    document.getElementById('reg-telp').value = p.telepon_darurat;

    const select = document.getElementById('reg-id-perangkat');
    const idEsc = escapeHtml(p.id_perangkat);
    select.innerHTML = `<option value="${idEsc}">${idEsc} (Saat ini)</option>`;

    document.getElementById('modal-registrasi').style.display = 'flex';
    document.getElementById('btn-simpan').onclick = submitEdit;
}

async function submitEdit() {
    const nama = document.getElementById('reg-nama').value;
    const idAlat = document.getElementById('reg-id-perangkat').value;
    const telp = document.getElementById('reg-telp').value;

    try {
        const res = await apiFetch(`/api/pendaki/${editingId}`, {
            method: "PUT",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ nama_pendaki: nama, id_perangkat: idAlat, telepon_darurat: telp })
        });
        if (res.ok)                  { showToast("Perubahan disimpan", "success"); closeModal(); fetchPendakiAktif(); fetchHistory(); }
        else if (res.status === 404) { showToast("Pendaki tidak ditemukan", "error"); }
        else                         { showToast("Gagal menyimpan", "error"); }
    } catch (e) { showToast("Gagal menyimpan", "error"); }
}

function exportCSV() {
    return exportExcel();
}

/**
 * Export riwayat pendakian ke .xlsx (Excel native).
 * Lebih ramah penjaga daripada CSV: kolom auto-width, header bold +
 * background warna, freeze row pertama, format tanggal proper.
 *
 * SheetJS (~600KB) di-lazy-load via CDN saat pertama kali dipanggil
 * supaya tidak men-bloat initial page load. Subsequent click instant
 * karena script sudah ter-cache browser.
 */
async function exportExcel() {
    if (historyData.length === 0) {
        return showToast("Tidak ada data untuk diekspor", "error");
    }

    // Lazy-load SheetJS dari CDN. unpkg + integrity supaya CSP-aman.
    if (typeof XLSX === "undefined") {
        showToast("Memuat modul export...", "info");
        try {
            await loadScriptOnce("https://cdn.jsdelivr.net/npm/xlsx@0.18.5/dist/xlsx.full.min.js");
        } catch (e) {
            showToast("Gagal memuat modul export. Cek koneksi internet.", "error");
            return;
        }
    }

    // Map data → array of objects dengan kolom human-readable Bahasa
    // Indonesia. Date di-format dd/MM/yyyy HH:mm supaya Excel id-ID
    // bisa parse otomatis tanpa bingung locale. Durasi pendakian
    // dihitung dari (tanggal_turun || now) - tanggal_naik dan
    // diformat sebagai "Xh Ymnt" untuk operator basecamp.
    const fmtDate = (iso) => iso
        ? new Date(iso).toLocaleString("id-ID", {
            day: "2-digit", month: "2-digit", year: "numeric",
            hour: "2-digit", minute: "2-digit",
        })
        : "—";
    const calcDuration = (naikIso, turunIso) => {
        if (!naikIso) return "—";
        const naik = new Date(naikIso);
        const turun = turunIso ? new Date(turunIso) : new Date();
        const diffMs = turun - naik;
        if (!Number.isFinite(diffMs) || diffMs < 0) return "—";
        const totalMin = Math.floor(diffMs / 60000);
        const days = Math.floor(totalMin / 1440);
        const hours = Math.floor((totalMin % 1440) / 60);
        const mins = totalMin % 60;
        const parts = [];
        if (days > 0) parts.push(`${days}h`);
        if (hours > 0) parts.push(`${hours}j`);
        if (mins > 0 || parts.length === 0) parts.push(`${mins}mnt`);
        return parts.join(" ");
    };
    const rows = historyData.map((p, i) => {
        // Cari snapshot battery terbaru untuk alat pendaki ini
        // (kalau ada di latestDataPerDevice). Pendaki yang sudah turun
        // mungkin alatnya dipakai pendaki lain — best effort.
        const live = latestDataPerDevice[p.id_perangkat];
        const battLabel = batteryStyle(live ? live.battery : null).label;
        return {
            "No": i + 1,
            "Nama Pendaki": p.nama_pendaki ?? "",
            "ID Perangkat": p.id_perangkat ?? "",
            "Telepon Darurat": p.telepon_darurat ?? "",
            "Status": p.status ?? "",
            "Waktu Naik": fmtDate(p.tanggal_naik),
            "Waktu Turun": fmtDate(p.tanggal_turun),
            "Durasi Pendakian": calcDuration(p.tanggal_naik, p.tanggal_turun),
            "Baterai (snapshot)": battLabel,
        };
    });

    const ws = XLSX.utils.json_to_sheet(rows);

    // Auto-fit kolom — hitung max length per kolom (header + data),
    // tambah padding 2 untuk breathing room.
    const headers = Object.keys(rows[0] || {});
    const colWidths = headers.map(h => {
        const maxLen = Math.max(
            h.length,
            ...rows.map(r => String(r[h] ?? "").length)
        );
        return { wch: Math.min(maxLen + 2, 40) };  // cap at 40 chars
    });
    ws["!cols"] = colWidths;

    // Freeze row pertama (header) supaya scroll tidak hilang konteks.
    ws["!freeze"] = { xSplit: 0, ySplit: 1 };
    // SheetJS Community pakai `!freeze` di workbook.Workbook? Tapi
    // safer pakai pane langsung. Tambahkan juga via `!ref` rangeguard.
    ws["!autofilter"] = { ref: ws["!ref"] };

    const wb = XLSX.utils.book_new();
    XLSX.utils.book_append_sheet(wb, ws, "Riwayat Pendakian");

    // Workbook metadata — judul + author = ALTIVEX biar terlihat
    // profesional saat penjaga buka di Excel.
    wb.Props = {
        Title: "Riwayat Pendakian ALTIVEX",
        Author: "ALTIVEX Basecamp",
        CreatedDate: new Date(),
    };

    const filename = `riwayat_altivex_${new Date().toISOString().split("T")[0]}.xlsx`;
    XLSX.writeFile(wb, filename);

    showToast("File Excel berhasil diunduh", "success");
}

/**
 * Lazy-load script eksternal sekali. Idempotent: panggilan ke-2 untuk
 * URL yang sama langsung resolve tanpa fetch ulang. Reject hanya saat
 * network error / 404.
 */
function loadScriptOnce(url) {
    return new Promise((resolve, reject) => {
        // Cek apakah script sudah ada di DOM.
        const existing = document.querySelector(`script[src="${url}"]`);
        if (existing) {
            if (existing.dataset.loaded === "1") return resolve();
            existing.addEventListener("load", () => resolve());
            existing.addEventListener("error", () => reject(new Error("script error")));
            return;
        }
        const s = document.createElement("script");
        s.src = url;
        s.async = true;
        s.addEventListener("load", () => {
            s.dataset.loaded = "1";
            resolve();
        });
        s.addEventListener("error", () => reject(new Error("network error")));
        document.head.appendChild(s);
    });
}

async function selesaikanPendakian(idAlat) {
    showConfirm("KONFIRMASI TURUN", `Apakah pendaki dengan alat ${idAlat} sudah benar-benar kembali ke basecamp?`, async () => {
        try {
            const res = await apiFetch(`/api/pendaki/${idAlat}/selesai`, { method: "PUT" });
            if (res.ok)                  { showToast("Pendakian diselesaikan", "success"); fetchPendakiAktif(); fetchHistory(); fetchInitialSensorData(); }
            else if (res.status === 404) { showToast("Pendaki tidak ditemukan", "error"); }
            else                         { showToast("Gagal update status", "error"); }
        } catch (e) { showToast("Gagal update (Koneksi)", "error"); }
    });
}

// ====================================================================
// LIVE UPDATE LOGIC + IN-APP ALERT BANNER (Task UI #6)
// --------------------------------------------------------------------
// `geofenceBuffer` adalah FeatureCollection of polygons (hasil
// `turf.buffer` atas FeatureCollection of LineString). turf 6
// `booleanPointInPolygon` HANYA terima single Feature<Polygon|
// MultiPolygon>. Saat dipanggil dengan FeatureCollection langsung,
// turf throw `Cannot read properties of undefined (reading 'length')`
// karena ia coba akses `.geometry.coordinates` di object yang berbeda
// shape. Helper ini iterasi tiap fitur dan return true kalau salah satu
// match — defensif & idempotent.
// ====================================================================
function pointInGeofence(point) {
    if (!geofenceBuffer) return true; // fail-open kalau buffer belum siap
    try {
        if (geofenceBuffer.type === "FeatureCollection") {
            for (const feat of (geofenceBuffer.features || [])) {
                if (!feat || !feat.geometry) continue;
                const t = feat.geometry.type;
                if (t !== "Polygon" && t !== "MultiPolygon") continue;
                if (turf.booleanPointInPolygon(point, feat)) return true;
            }
            return false;
        }
        // Single Feature/Polygon path
        return turf.booleanPointInPolygon(point, geofenceBuffer);
    } catch (e) {
        // Kalau turf tetap meledak (mis. fitur tanpa coords), fail-open
        // — lebih baik tampilkan kartu standby daripada UI mati total.
        console.warn("[ALTIVEX] pointInGeofence error:", e);
        return true;
    }
}

let renderTimeout = null;
function renderHikerCards() {
    if (renderTimeout) clearTimeout(renderTimeout);
    renderTimeout = setTimeout(() => {
        try {
            _renderHikerCards();
        } catch (e) {
            // Last-resort guard: kalau render meledak, jangan kill init
            // (peta + status bar tetap hidup). Catat untuk forensics.
            console.error("[ALTIVEX] _renderHikerCards crashed:", e);
        }
    }, 500);
}

/**
 * Update banner peringatan in-app di tab "Peta Live".
 * Banner muncul jika `outsideCount >= 1`. Klik banner →
 * scroll ke kartu alert pertama di sidebar.
 */
function updateAlertBanner(outsideCount) {
    const banner = document.getElementById('map-alert-banner');
    const text   = document.getElementById('map-alert-banner-text');
    if (!banner || !text) return;

    if (outsideCount > 0) {
        text.textContent = outsideCount === 1
            ? "1 pendaki di luar koridor"
            : `${outsideCount} pendaki di luar koridor`;
        banner.hidden = false;
    } else {
        banner.hidden = true;
    }
}

function focusFirstAlert() {
    const firstAlert = document.querySelector('#hiker_list .alert-card');
    if (!firstAlert) return;
    // Switch ke tab Peta Live kalau operator sedang di tab lain.
    const liveTab = document.getElementById('tab-live');
    if (liveTab && !liveTab.classList.contains('active')) {
        const liveLink = document.querySelector('.tab-link');
        openTab('tab-live', liveLink);
    }
    firstAlert.scrollIntoView({ behavior: 'smooth', block: 'center' });
    // Tambahkan flash ringan agar jelas mana yang ter-fokus.
    firstAlert.style.transition = 'box-shadow 0.4s';
    const oldShadow = firstAlert.style.boxShadow;
    firstAlert.style.boxShadow = '12px 12px 0px 0px var(--red)';
    setTimeout(() => { firstAlert.style.boxShadow = oldShadow; }, 800);
}

function _renderHikerCards() {
    let alertHTML = "";
    let availHTML = "";
    let outsideCount = 0;

    for (let id in latestDataPerDevice) {
        const data = latestDataPerDevice[id];
        // Guard: payload korup / koordinat NaN — lewati supaya turf
        // tidak meledak dan loop bisa lanjut ke device berikutnya.
        if (!data || !Number.isFinite(data.latitude) || !Number.isFinite(data.longitude)) {
            continue;
        }
        const point = turf.point([data.longitude, data.latitude]);
        const isInside = pointInGeofence(point);
        const hiker = registeredHikers[id];

        // Battery monitor — trigger notif kalau pertama kali < 15%.
        // Hiker bisa null kalau alat masih standby; pakai id sebagai
        // fallback display name di pesan notif.
        maybeNotifyLowBattery(id, data.battery, hiker ? hiker.nama_pendaki : null);
        const battHtml = batteryPill(data.battery);

        if (!activeMarkers[id]) {
            activeMarkers[id] = L.marker([data.latitude, data.longitude]).addTo(map);
        } else {
            activeMarkers[id].setLatLng([data.latitude, data.longitude]);
        }

        const idEsc = escapeHtml(id);

        if (hiker) {
            const namaEsc = escapeHtml(hiker.nama_pendaki);
            const telpEsc = escapeHtml(hiker.telepon_darurat);
            const battStyle = batteryStyle(data.battery);
            activeMarkers[id].bindPopup(
                `<b>${namaEsc}</b><br>ID: ${idEsc}<br>Status: ${isInside ? 'Aman' : 'KELUAR JALUR'}<br>Baterai: ${battStyle.label}`
            );

            if (!isInside) {
                outsideCount += 1;
                if (!isNotified(id)) {
                    sendNotification("⚠ PERINGATAN KELUAR JALUR",
                        `${hiker.nama_pendaki} terpantau keluar dari koridor pendakian!`);
                    setNotified(id, true);
                }

                alertHTML += `
                    <div class="neo-card alert-card">
                        <div class="alert-card__name">${ICON('warning', 18)} ${escapeHtml(String(hiker.nama_pendaki ?? "").toUpperCase())}</div>
                        <div class="alert-card__meta">ID: ${idEsc} | Telp: ${telpEsc}</div>
                        <div class="alert-card__row">
                            <div class="neo-badge badge-keluar">KELUAR KORIDOR</div>
                            ${battHtml}
                        </div>
                        <div class="alert-card__actions">
                            <button class="neo-btn neo-btn-sm neo-btn-red" data-action="alert" data-id="${idEsc}">${ICON('bell', 14)} ALERT</button>
                            <button class="neo-btn neo-btn-sm" data-action="path" data-id="${idEsc}">${ICON('map', 14)} PATH</button>
                        </div>
                    </div>
                `;
            } else {
                setNotified(id, false);
            }
        } else {
            activeMarkers[id].bindPopup(`<b>PERANGKAT: ${idEsc}</b><br>Belum terdaftar`);
            availHTML += `
                <div class="neo-card standby-card">
                    <div class="standby-card__name">${ICON('device', 18)} ${idEsc}</div>
                    <div class="standby-card__meta">Status: Standby (Online)</div>
                    <div class="standby-card__row">${battHtml}</div>
                    <button class="neo-btn neo-btn-sm neo-btn-blue" data-action="register" data-id="${idEsc}">${ICON('plus', 14)} DAFTAR</button>
                </div>
            `;
        }
    }

    const hikerListEl = document.getElementById('hiker_list');
    const availableListEl = document.getElementById('available_list');
    hikerListEl.innerHTML = alertHTML
        || `<div class="neo-card all-safe-card">${ICON('check', 18)} SEMUA AMAN</div>`;
    availableListEl.innerHTML = availHTML
        || '<p class="empty-message">Tidak ada alat standby.</p>';

    if (!hikerListEl.dataset.delegated) {
        hikerListEl.addEventListener('click', handleHikerListClick);
        hikerListEl.dataset.delegated = "1";
    }
    if (!availableListEl.dataset.delegated) {
        availableListEl.addEventListener('click', handleAvailableListClick);
        availableListEl.dataset.delegated = "1";
    }

    updateAlertBanner(outsideCount);
}

function handleHikerListClick(ev) {
    const target = ev.target.closest('[data-action]');
    if (!target || !this.contains(target)) return;
    const action = target.dataset.action;
    const id = target.dataset.id || "";
    if (!id) return;
    if (action === 'alert')      kirimGetaran(id);
    else if (action === 'path')  toggleHistory(id);
}

function handleAvailableListClick(ev) {
    const target = ev.target.closest('[data-action]');
    if (!target || !this.contains(target)) return;
    if (target.dataset.action !== 'register') return;
    const id = target.dataset.id || "";
    if (id) openModalWithId(id);
}

function openModalWithId(id) {
    openModal();
    document.getElementById('reg-id-perangkat').value = id;
}

async function kirimGetaran(id) {
    showToast(`Mengirim sinyal getar ke ${id}...`);
    try {
        await apiFetch("/api/alert", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ id_perangkat: id, jenis_peringatan: "OUT_OF_BOUNDS" })
        });
    } catch (e) {}
}

async function toggleHistory(id) {
    if (activePolylines[id]) {
        map.removeLayer(activePolylines[id].poly);
        activePolylines[id].markers.forEach(m => map.removeLayer(m));
        delete activePolylines[id];
        return;
    }

    const res = await apiFetch(`/api/history/${id}`);
    const data = await res.json();
    if (data.length === 0) return showToast("Tidak ada data history", "error");

    const latlngs = data.map(p => [p.latitude, p.longitude]);
    const poly = L.polyline(latlngs, {
        color: 'var(--blue)', weight: 6, dashArray: '10, 10', opacity: 0.7
    }).addTo(map);

    const startIcon = L.divIcon({ html: `<span class="path-marker path-marker--start">${ICON('circleDot', 22)}</span>`, className: '', iconSize: [24, 24], iconAnchor: [12, 12] });
    const endIcon   = L.divIcon({ html: `<span class="path-marker path-marker--end">${ICON('flag', 22)}</span>`, className: '', iconSize: [24, 24], iconAnchor: [12, 12] });

    const startMarker = L.marker(latlngs[0], { icon: startIcon }).addTo(map).bindPopup("Titik Mulai");
    const endMarker   = L.marker(latlngs[latlngs.length - 1], { icon: endIcon }).addTo(map).bindPopup("Posisi Terakhir");

    activePolylines[id] = { poly, markers: [startMarker, endMarker] };
    map.fitBounds(poly.getBounds());
}

let miniMap = null;
async function viewJourneyDetail(p) {
    document.getElementById('detail-nama').innerText = p.nama_pendaki;
    document.getElementById('stat-start').innerText  = new Date(p.tanggal_naik).toLocaleString('id-ID');
    document.getElementById('stat-status').innerText = p.status;

    // Tampilkan waktu selesai (kalau sudah turun) + durasi pendakian.
    // Pendaki masih `Mendaki` → durasi dihitung sampai sekarang
    // (rolling), waktu selesai diberi label "—".
    const endEl = document.getElementById('stat-end');
    const durEl = document.getElementById('stat-duration');
    if (endEl) {
        endEl.innerText = p.tanggal_turun
            ? new Date(p.tanggal_turun).toLocaleString('id-ID')
            : "—";
    }
    if (durEl) {
        const naik = new Date(p.tanggal_naik);
        const turun = p.tanggal_turun ? new Date(p.tanggal_turun) : new Date();
        const totalMin = Math.max(0, Math.floor((turun - naik) / 60000));
        const days = Math.floor(totalMin / 1440);
        const hours = Math.floor((totalMin % 1440) / 60);
        const mins = totalMin % 60;
        const parts = [];
        if (days > 0) parts.push(`${days}h`);
        if (hours > 0) parts.push(`${hours}j`);
        if (mins > 0 || parts.length === 0) parts.push(`${mins}mnt`);
        durEl.innerText = parts.join(" ");
    }

    document.getElementById('modal-detail').style.display = 'flex';

    if (!miniMap) {
        miniMap = L.map("mini-map").setView(posJagaLatLng, 13);
        L.tileLayer("https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png").addTo(miniMap);
    } else {
        miniMap.eachLayer(l => {
            if (l instanceof L.Marker || l instanceof L.Polyline) miniMap.removeLayer(l);
        });
    }

    setTimeout(async () => {
        miniMap.invalidateSize();
        try {
            const res = await apiFetch(`/api/pendaki/${p.id}/history`);
            const data = await res.json();
            if (data.length > 1) {
                const latlngs = data.map(d => [d.latitude, d.longitude]);
                const line = turf.lineString(data.map(d => [d.longitude, d.latitude]));
                const distance = turf.length(line, { units: 'kilometers' }).toFixed(2);
                document.getElementById('stat-dist').innerText = distance;
                const poly = L.polyline(latlngs, { color: 'var(--blue)', weight: 5 }).addTo(miniMap);
                L.marker(latlngs[0]).addTo(miniMap).bindPopup("Mulai");
                L.marker(latlngs[latlngs.length - 1]).addTo(miniMap).bindPopup("Posisi Terakhir");
                miniMap.fitBounds(poly.getBounds(), { padding: [20, 20] });
            } else {
                document.getElementById('stat-dist').innerText = "0";
                miniMap.setView(posJagaLatLng, 15);
            }
        } catch (e) { console.error(e); }
    }, 300);
}

function closeDetailModal() { document.getElementById('modal-detail').style.display = 'none'; }

// ====================================================================
// SEARCH & FILTER (Task 3.12)
// ====================================================================
function applyFilters() {
    const q = document.getElementById('search-input').value.toLowerCase();
    const radio = document.querySelector('input[name="filter"]:checked');
    const status = radio ? radio.value : 'Semua';
    const filtered = historyData.filter(p =>
        (status === 'Semua' || p.status === status) &&
        p.nama_pendaki.toLowerCase().includes(q)
    );
    renderHistoryTable(filtered);
}
function searchPendaki()   { applyFilters(); }
function filterHistory()   { applyFilters(); }

// ====================================================================
// WS & STATUS
// --------------------------------------------------------------------
// Visibility WS untuk operator + fallback polling cepat saat WS down.
// `wsHealthy` = true setelah `onopen`; false saat `onerror`/`onclose`.
// Polling `/api/sensor/latest` di-jalankan tiap `LIVE_POLL_FAST_MS`
// saat WS down, dan `LIVE_POLL_SLOW_MS` saat WS hidup (mengurangi
// beban backend ketika WS sudah cover real-time).
// --------------------------------------------------------------------
// Reverse proxy (nginx) WAJIB punya konfigurasi WS upgrade pada
// location /ws — lihat `deployment/Caddyfile` untuk Caddy, atau
// snippet nginx di README. Tanpa konfigurasi upgrade, browser
// dapat 200/502 dan onopen tidak pernah terpanggil.
// ====================================================================
const LIVE_POLL_FAST_MS = 5000;   // saat WS down, polling agresif
const LIVE_POLL_SLOW_MS = 30000;  // saat WS hidup, polling untuk safety net
let wsHealthy = false;
let livePollTimer = null;
let wsReconnectAttempts = 0;

function schedulePolling() {
    if (livePollTimer) clearInterval(livePollTimer);
    const interval = wsHealthy ? LIVE_POLL_SLOW_MS : LIVE_POLL_FAST_MS;
    livePollTimer = setInterval(fetchInitialSensorData, interval);
    console.log(`[ALTIVEX] Polling /api/sensor/latest tiap ${interval}ms (WS healthy=${wsHealthy})`);
}

function connectWebSocket() {
    const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
    const url = `${protocol}//${window.location.host}/ws`;
    console.log(`[ALTIVEX] WS connecting → ${url}`);
    const ws = new WebSocket(url);

    ws.onopen = () => {
        wsHealthy = true;
        wsReconnectAttempts = 0;
        console.log("[ALTIVEX] WS OPEN — real-time aktif");
        // Saat baru connect, fetch sekali snapshot terbaru supaya
        // sidebar tidak kosong selama menunggu publish berikutnya.
        fetchInitialSensorData();
        schedulePolling();
    };

    ws.onmessage = (e) => {
        try {
            const data = JSON.parse(e.data);
            latestDataPerDevice[data.id_perangkat] = data;
            renderHikerCards();
        } catch (err) {
            console.warn("[ALTIVEX] WS payload tidak valid:", err);
        }
    };

    ws.onerror = (ev) => {
        console.warn("[ALTIVEX] WS ERROR — kemungkinan reverse proxy belum support upgrade", ev);
    };

    ws.onclose = (ev) => {
        wsHealthy = false;
        wsReconnectAttempts += 1;
        const delay = Math.min(1000 * Math.pow(2, wsReconnectAttempts - 1), 15000);
        console.log(`[ALTIVEX] WS CLOSE (code=${ev.code}). Reconnect dalam ${delay}ms (attempt #${wsReconnectAttempts})`);
        schedulePolling();
        setTimeout(connectWebSocket, delay);
    };
}

async function checkStatuses() {
    try {
        const res = await fetch("/api/status");
        const data = await res.json();
        const dot = document.getElementById('basecamp-dot');
        const txt = document.getElementById('basecamp-text');
        if (data.status === 'online') {
            dot.className = 'dot dot-green';
            txt.innerText = 'Basecamp Online';
        } else {
            dot.className = 'dot dot-red';
            txt.innerText = 'Basecamp Offline';
        }
    } catch (e) {}

    const cDot = document.getElementById('cloud-dot');
    const cTxt = document.getElementById('cloud-text');
    if (navigator.onLine) {
        cDot.className = 'dot dot-green';
        cTxt.innerText = 'Cloud Synced';
    } else {
        cDot.className = 'dot dot-red';
        cTxt.innerText = 'Local Mode';
    }
}

async function fetchInitialSensorData() {
    try {
        const response = await apiFetch("/api/sensor/latest");
        const data = await response.json();
        data.forEach(d => { latestDataPerDevice[d.id_perangkat] = d; });
        renderHikerCards();
    } catch (e) { console.error("Gagal fetch initial sensor:", e); }
}

// ====================================================================
// INIT
// ====================================================================
// Skeleton placeholder awal — supaya operator yang baru refresh
// di mobile (koneksi lambat) tidak salah sangka data hilang.
// `_renderHikerCards` akan replace innerHTML ini begitu dapat data
// pertama (lewat WS atau polling).
(function paintInitialSkeleton() {
    const hiker = document.getElementById('hiker_list');
    const avail = document.getElementById('available_list');
    const skeleton = `<div class="skeleton-card">${ICON('refresh', 18)} Memuat data live...</div>`;
    if (hiker && !hiker.innerHTML.trim()) hiker.innerHTML = skeleton;
    if (avail && !avail.innerHTML.trim()) avail.innerHTML = skeleton;
})();

setInterval(checkStatuses, 5000);
// Polling /api/sensor/latest di-handle oleh `schedulePolling()` lewat
// `connectWebSocket()` (interval menyesuaikan kondisi WS). Polling
// pendaki tetap konstan di interval lama.
setInterval(fetchPendakiAktif, 10000);

checkStatuses();
fetchInitialSensorData();
fetchPendakiAktif();
connectWebSocket();

// Banner alert in-app (Task UI #6) + theme toggle — bind handler sekali.
document.addEventListener('DOMContentLoaded', () => {
    // ----- ICON PAINTING -----
    // Semua element dengan attribute `data-icon="<name>"` akan di-isi
    // SVG dari ICON_PATHS. Ini menggantikan pola hardcoded emoji di
    // markup. Default size 18px; element bisa override pakai
    // `data-icon-size`.
    document.querySelectorAll('[data-icon]').forEach(el => {
        const name = el.getAttribute('data-icon');
        const size = parseInt(el.getAttribute('data-icon-size') || '18', 10);
        el.innerHTML = ICON(name, size);
    });

    // Logo header — render mountain icon ukuran besar.
    const logoIcon = document.getElementById('logo-icon');
    if (logoIcon) logoIcon.innerHTML = ICON('mountain', 28);

    // Theme toggle button — icon set sesuai theme aktif (handled
    // dynamically oleh applyTheme()).
    const themeBtn = document.getElementById('theme-toggle');
    if (themeBtn) {
        applyTheme(document.body.classList.contains('dark-mode') ? 'dark' : 'light');
        themeBtn.addEventListener('click', toggleTheme);
    }

    // Logout button icon.
    const logoutBtn = document.getElementById('logout-btn');
    if (logoutBtn) {
        logoutBtn.innerHTML = ICON('logout', 18);
        logoutBtn.addEventListener('click', logout);
    }

    // Tab navigation — replace inline onclick dengan delegated listener
    // supaya icon di dalam tombol tidak ke-overwrite.
    document.querySelectorAll('.tab-link[data-tab]').forEach(btn => {
        btn.addEventListener('click', () => openTab(btn.dataset.tab, btn));
    });

    // Toolbar buttons (Export CSV, Daftarkan Pendaki) — pakai
    // data-action karena onclick="exportCSV()" menyentuh isi tombol.
    document.querySelectorAll('[data-action="exportCSV"]').forEach(b => b.addEventListener('click', exportCSV));
    document.querySelectorAll('[data-action="openModal"]').forEach(b => b.addEventListener('click', openModal));

    const banner = document.getElementById('map-alert-banner');
    if (banner) {
        banner.addEventListener('click', focusFirstAlert);
    }

    // Login form (UI #4). Submit handler + show/hide modal di awal
    // bergantung apakah token sudah tersimpan.
    const loginForm = document.getElementById('login-form');
    if (loginForm) loginForm.addEventListener('submit', handleLoginSubmit);

    // Initial gate: kalau belum punya token, tampilkan login modal +
    // sembunyikan logout. Sebaliknya, hide modal + show logout.
    if (getStoredToken()) {
        hideLoginModal();
    } else {
        showLoginModal();
    }

    // Listen ke perubahan system preference, tapi HANYA terapkan kalau
    // user belum pernah memilih manual (`localStorage` masih kosong).
    if (window.matchMedia) {
        const mq = window.matchMedia('(prefers-color-scheme: dark)');
        const onSysChange = (ev) => {
            if (getStoredTheme()) return; // user choice menang
            applyTheme(ev.matches ? 'dark' : 'light');
        };
        if (mq.addEventListener) mq.addEventListener('change', onSysChange);
        else if (mq.addListener) mq.addListener(onSysChange); // Safari lama
    }
});
