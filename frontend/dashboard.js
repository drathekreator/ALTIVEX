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
// API Auth Token (Task 3.8 — Bug B3)
// ====================================================================
const TOKEN_STORAGE_KEY = "ALTIVEX_API_TOKEN";

function getApiToken() {
    let t = null;
    try { t = localStorage.getItem(TOKEN_STORAGE_KEY); } catch (e) { t = null; }
    if (!t) {
        const input = window.prompt("Masukkan API token ALTIVEX:");
        if (input && input.trim()) {
            t = input.trim();
            try { localStorage.setItem(TOKEN_STORAGE_KEY, t); } catch (e) {}
        } else {
            t = "";
        }
    }
    return t || "";
}

let __altivexTokenReprompted = false;
function clearApiToken() {
    try { localStorage.removeItem(TOKEN_STORAGE_KEY); } catch (e) {}
}

async function apiFetch(url, options) {
    const opts = options ? Object.assign({}, options) : {};
    const headers = new Headers(opts.headers || {});
    const token = getApiToken();
    if (token) headers.set("Authorization", "Bearer " + token);
    opts.headers = headers;

    let res = await fetch(url, opts);
    if (res.status === 401 && !__altivexTokenReprompted) {
        __altivexTokenReprompted = true;
        clearApiToken();
        if (typeof showToast === "function") {
            showToast("⚠️ Token tidak valid, silakan masukkan ulang", "error");
        }
        const newToken = getApiToken();
        if (newToken) {
            const headers2 = new Headers((options && options.headers) || {});
            headers2.set("Authorization", "Bearer " + newToken);
            const opts2 = options ? Object.assign({}, options, { headers: headers2 }) : { headers: headers2 };
            res = await fetch(url, opts2);
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

// GEO.json state
let geoData = null;
let routeFeatures = null;
let waypointFeatures = null;

const routeColors = {
    'Cibodas':       '#2979FF',
    'Gunung Putri':  '#FF6D00',
    'Selabintana':   '#AA00FF'
};

const waypointIcons = {
    'Trailhead': '🚩', 'Gate': '🚪', 'Pos': '🏠', 'Camp': '⛺',
    'Summit': '🏔️', 'Junction': '🔀', 'Waypoint': '📍',
    'Water': '💧', 'default': '📌'
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
            const icon = waypointIcons[props.type] || waypointIcons['default'];
            const routeColor = routeColors[props.route] || '#333';

            const marker = L.marker([lat, lng], {
                icon: L.divIcon({
                    html: `<span class="waypoint-icon">${icon}</span>`,
                    className: '',
                    iconSize: [24, 24],
                    iconAnchor: [12, 12]
                })
            }).addTo(map);

            const elev = props.elevation_m ? `${props.elevation_m} mdpl` : '-';
            marker.bindPopup(
                `<div style="font-family:Outfit,sans-serif;">` +
                `<b style="font-size:14px;">${icon} ${escapeHtml(props.name)}</b><br>` +
                `<span style="background:${routeColor}; color:white; padding:2px 8px; font-size:11px; font-weight:700; border:2px solid #000;">` +
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
        btn.textContent = isDark ? "☀" : "🌙";
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
    // wrapper innerHTML untuk emoji, dan teks lewat textContent supaya
    // string apa pun (mis. dari error message backend) tidak ter-parse
    // sebagai HTML.
    toast.innerHTML = '<span>🔔</span> ';
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
            ? `<button class="neo-btn neo-btn-sm neo-btn-blue" data-action="finish" data-id-perangkat="${idPerangkatAttr}">✅ Selesai</button>`
            : `<button class="neo-btn neo-btn-sm neo-btn-red" data-action="delete" data-id="${idAttr}">🗑️</button>`;
        return `
            <tr>
                <td>${escapeHtml(p.nama_pendaki)}</td>
                <td><span class="neo-badge badge-id">${escapeHtml(p.id_perangkat)}</span></td>
                <td class="hide-mobile">${escapeHtml(p.telepon_darurat)}</td>
                <td>
                    <span class="neo-badge ${statusBadgeCls}">${escapeHtml(p.status)}</span>
                </td>
                <td class="hide-mobile">${escapeHtml(new Date(p.tanggal_naik).toLocaleString('id-ID'))}</td>
                <td>
                    <div class="history-actions">
                        ${actionBtns}
                        <button class="neo-btn neo-btn-sm" data-action="edit" data-id="${idAttr}">✏️</button>
                        <button class="neo-btn neo-btn-sm neo-btn-green" data-action="view" data-id="${idAttr}">🗺️</button>
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
            if (res.ok)                    { showToast("✅ Data dihapus", "success"); fetchHistory(); }
            else if (res.status === 404)   { showToast("❌ Pendaki tidak ditemukan", "error"); }
            else                           { showToast("❌ Gagal menghapus", "error"); }
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
        if (res.ok)                  { showToast("✅ Perubahan Disimpan!", "success"); closeModal(); fetchPendakiAktif(); fetchHistory(); }
        else if (res.status === 404) { showToast("❌ Pendaki tidak ditemukan", "error"); }
        else                         { showToast("❌ Gagal menyimpan", "error"); }
    } catch (e) { showToast("Gagal menyimpan", "error"); }
}

function exportCSV() {
    if (historyData.length === 0) return showToast("Tidak ada data untuk dieksport", "error");

    const headers = ["Nama", "ID Alat", "Telepon", "Status", "Waktu Naik"];
    const rows = historyData.map(p => [
        p.nama_pendaki, p.id_perangkat, p.telepon_darurat, p.status, p.tanggal_naik
    ].map(csvField).join(","));

    const body = headers.join(",") + "\r\n" + rows.join("\r\n") + "\r\n";
    const bom = "\uFEFF";
    const blob = new Blob([bom + body], { type: "text/csv;charset=utf-8;" });

    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.setAttribute("href", url);
    link.setAttribute("download", `riwayat_altivex_${new Date().toISOString().split('T')[0]}.csv`);
    document.body.appendChild(link);
    link.click();
    link.remove();
    URL.revokeObjectURL(url);
}

async function selesaikanPendakian(idAlat) {
    showConfirm("KONFIRMASI TURUN", `Apakah pendaki dengan alat ${idAlat} sudah benar-benar kembali ke basecamp?`, async () => {
        try {
            const res = await apiFetch(`/api/pendaki/${idAlat}/selesai`, { method: "PUT" });
            if (res.ok)                  { showToast("✅ Pendakian diselesaikan", "success"); fetchPendakiAktif(); fetchHistory(); fetchInitialSensorData(); }
            else if (res.status === 404) { showToast("❌ Pendaki tidak ditemukan", "error"); }
            else                         { showToast("❌ Gagal update status", "error"); }
        } catch (e) { showToast("❌ Gagal update (Koneksi)", "error"); }
    });
}

// ====================================================================
// LIVE UPDATE LOGIC + IN-APP ALERT BANNER (Task UI #6)
// ====================================================================
let renderTimeout = null;
function renderHikerCards() {
    if (renderTimeout) clearTimeout(renderTimeout);
    renderTimeout = setTimeout(_renderHikerCards, 500);
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
        const point = turf.point([data.longitude, data.latitude]);
        const isInside = geofenceBuffer ? turf.booleanPointInPolygon(point, geofenceBuffer) : true;
        const hiker = registeredHikers[id];

        if (!activeMarkers[id]) {
            activeMarkers[id] = L.marker([data.latitude, data.longitude]).addTo(map);
        } else {
            activeMarkers[id].setLatLng([data.latitude, data.longitude]);
        }

        const idEsc = escapeHtml(id);

        if (hiker) {
            const namaEsc = escapeHtml(hiker.nama_pendaki);
            const telpEsc = escapeHtml(hiker.telepon_darurat);
            activeMarkers[id].bindPopup(
                `<b>${namaEsc}</b><br>ID: ${idEsc}<br>Status: ${isInside ? 'Aman' : 'KELUAR JALUR'}`
            );

            if (!isInside) {
                outsideCount += 1;
                if (!isNotified(id)) {
                    sendNotification("⚠️ PERINGATAN KELUAR JALUR",
                        `${hiker.nama_pendaki} terpantau keluar dari koridor pendakian!`);
                    setNotified(id, true);
                }

                alertHTML += `
                    <div class="neo-card alert-card">
                        <div class="alert-card__name">⚠️ ${escapeHtml(String(hiker.nama_pendaki ?? "").toUpperCase())}</div>
                        <div class="alert-card__meta">ID: ${idEsc} | Telp: ${telpEsc}</div>
                        <div class="neo-badge badge-keluar">KELUAR KORIDOR</div>
                        <div class="alert-card__actions">
                            <button class="neo-btn neo-btn-sm neo-btn-red" data-action="alert" data-id="${idEsc}">🔔 ALERT</button>
                            <button class="neo-btn neo-btn-sm" data-action="path" data-id="${idEsc}">🗺️ PATH</button>
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
                    <div class="standby-card__name">🛰️ ${idEsc}</div>
                    <div class="standby-card__meta">Status: Standby (Online)</div>
                    <button class="neo-btn neo-btn-sm neo-btn-blue" data-action="register" data-id="${idEsc}">📝 DAFTAR</button>
                </div>
            `;
        }
    }

    const hikerListEl = document.getElementById('hiker_list');
    const availableListEl = document.getElementById('available_list');
    hikerListEl.innerHTML = alertHTML
        || '<div class="neo-card all-safe-card">✅ SEMUA AMAN</div>';
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

    const startIcon = L.divIcon({ html: '🟢', className: '', iconSize: [20, 20], iconAnchor: [10, 10] });
    const endIcon   = L.divIcon({ html: '🚩', className: '', iconSize: [20, 20], iconAnchor: [10, 10] });

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
    const banner = document.getElementById('map-alert-banner');
    if (banner) {
        banner.addEventListener('click', focusFirstAlert);
    }

    // Theme toggle (Modern Warm + dark mode). Re-apply ke tombol agar
    // icon/aria sesuai keadaan saat DOM sudah siap.
    const themeBtn = document.getElementById('theme-toggle');
    if (themeBtn) {
        applyTheme(document.body.classList.contains('dark-mode') ? 'dark' : 'light');
        themeBtn.addEventListener('click', toggleTheme);
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
