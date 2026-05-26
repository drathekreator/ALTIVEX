/* =====================================================================
   ALTIVEX Icon Library — neobrutalism monoline set
   =====================================================================
   Replace emoji yang sebelumnya bertebaran di markup dengan SVG icon
   konsisten:
   - viewBox 24×24 (standard)
   - stroke 2.5px (tebal khas neobrutalism)
   - stroke-linecap=round, stroke-linejoin=round
   - currentColor (auto adapt theme: light/dark, hover, badge fill)
   - fill="none" kecuali untuk dot/badge solid

   Pemakaian:
     // Inline string (cocok untuk innerHTML / template literal)
     hikerListEl.innerHTML += `<button>${ICON('alert', 18)} ALERT</button>`;

     // DOM Node (cocok untuk createElement-based code)
     const node = iconNode('mountain', 32);
     header.appendChild(node);

   Re-design / re-export:
   - Source SVG individual ada di `/icon/<name>.svg` untuk diedit di
     Figma. JS registry di sini adalah inline copy yang dimuat browser.
   - Setelah edit Figma, copy path-nya ke entry yang sesuai di
     `ICON_PATHS` di bawah.
   ===================================================================== */

const ICON_PATHS = {
    // ---------- BRAND ----------
    // Wordmark "ALTIVEX" disertakan untuk header (separate dari logo
    // mountain). Untuk header kompak kita pakai mountain saja.
    mountain: `
        <path d="M2 20 L8 10 L12 14 L18 4 L22 20 Z" />
        <path d="M14 11 L16 8 L18 10" />
    `,

    // ---------- THEME ----------
    moon: `
        <path d="M21 13.5 A9 9 0 0 1 10.5 3 A7.5 7.5 0 1 0 21 13.5 Z" />
    `,
    sun: `
        <circle cx="12" cy="12" r="4" />
        <path d="M12 2 V4 M12 20 V22 M2 12 H4 M20 12 H22 M5 5 L6.5 6.5 M17.5 17.5 L19 19 M5 19 L6.5 17.5 M17.5 6.5 L19 5" />
    `,

    // ---------- AUTH ----------
    unlock: `
        <rect x="4" y="11" width="16" height="10" rx="2" />
        <path d="M8 11 V7 A4 4 0 0 1 16 6.5" />
        <circle cx="12" cy="16" r="1.5" fill="currentColor" stroke="none" />
    `,
    logout: `
        <path d="M14 4 H19 A2 2 0 0 1 21 6 V18 A2 2 0 0 1 19 20 H14" />
        <path d="M9 16 L4 12 L9 8" />
        <path d="M4 12 H15" />
    `,
    user: `
        <circle cx="12" cy="8" r="4" />
        <path d="M4 21 V20 A6 6 0 0 1 10 14 H14 A6 6 0 0 1 20 20 V21" />
    `,
    users: `
        <circle cx="9" cy="8" r="3.5" />
        <path d="M2 20 V19 A5 5 0 0 1 7 14 H11 A5 5 0 0 1 16 19 V20" />
        <circle cx="17" cy="9" r="3" />
        <path d="M16 14 H18 A4 4 0 0 1 22 18 V19" />
    `,

    // ---------- ALERTS ----------
    bell: `
        <path d="M6 9 A6 6 0 0 1 18 9 V13 L20 16 H4 L6 13 Z" />
        <path d="M10 19 A2 2 0 0 0 14 19" />
    `,
    bellAlert: `
        <path d="M6 9 A6 6 0 0 1 18 9 V13 L20 16 H4 L6 13 Z" />
        <path d="M10 19 A2 2 0 0 0 14 19" />
        <circle cx="18.5" cy="6" r="2" fill="currentColor" stroke="none" />
    `,
    warning: `
        <path d="M12 3 L22 20 H2 Z" />
        <path d="M12 9 V14" />
        <circle cx="12" cy="17" r="1.2" fill="currentColor" stroke="none" />
    `,
    check: `
        <circle cx="12" cy="12" r="9" />
        <path d="M8 12 L11 15 L16 9" />
    `,
    checkSimple: `
        <path d="M5 12 L10 17 L19 7" />
    `,
    cross: `
        <circle cx="12" cy="12" r="9" />
        <path d="M9 9 L15 15 M15 9 L9 15" />
    `,
    info: `
        <circle cx="12" cy="12" r="9" />
        <path d="M12 11 V16" />
        <circle cx="12" cy="8" r="1" fill="currentColor" stroke="none" />
    `,

    // ---------- DEVICE / TELEMETRY ----------
    // Device standby — antena base + display
    device: `
        <rect x="6" y="10" width="12" height="11" rx="1.5" />
        <path d="M2 6 C7 3 17 3 22 6" />
        <path d="M5 9 C9 7 15 7 19 9" />
        <path d="M9 16 L12 14 L15 16" />
    `,
    broadcast: `
        <circle cx="12" cy="12" r="2" />
        <path d="M8.5 8.5 A5 5 0 0 0 8.5 15.5" />
        <path d="M15.5 8.5 A5 5 0 0 1 15.5 15.5" />
        <path d="M5.5 5.5 A9 9 0 0 0 5.5 18.5" />
        <path d="M18.5 5.5 A9 9 0 0 1 18.5 18.5" />
    `,
    satellite: `
        <path d="M3 21 L21 3" />
        <path d="M9 5 L19 15 A2 2 0 0 1 19 17 L17 19 A2 2 0 0 1 15 19 L5 9 A2 2 0 0 1 5 7 L7 5 A2 2 0 0 1 9 5 Z" />
        <circle cx="12" cy="12" r="1" fill="currentColor" stroke="none" />
    `,

    // ---------- ACTIONS ----------
    download: `
        <path d="M12 3 V15" />
        <path d="M7 11 L12 16 L17 11" />
        <path d="M4 19 H20" />
    `,
    upload: `
        <path d="M12 17 V5" />
        <path d="M7 9 L12 4 L17 9" />
        <path d="M4 19 H20" />
    `,
    plus: `
        <path d="M12 5 V19" />
        <path d="M5 12 H19" />
    `,
    edit: `
        <path d="M4 20 L8 19 L20 7 L17 4 L5 16 Z" />
        <path d="M14 7 L17 10" />
    `,
    trash: `
        <path d="M4 7 H20" />
        <path d="M9 7 V4 H15 V7" />
        <path d="M6 7 L7 21 H17 L18 7" />
        <path d="M10 11 V17 M14 11 V17" />
    `,
    map: `
        <path d="M3 6 L9 4 L15 6 L21 4 V18 L15 20 L9 18 L3 20 Z" />
        <path d="M9 4 V18" />
        <path d="M15 6 V20" />
    `,
    search: `
        <circle cx="11" cy="11" r="7" />
        <path d="M16 16 L21 21" />
    `,
    refresh: `
        <path d="M21 12 A9 9 0 1 1 18.5 6" />
        <path d="M21 4 V9 H16" />
    `,
    clock: `
        <circle cx="12" cy="12" r="9" />
        <path d="M12 7 V12 L15 14" />
    `,

    // ---------- WAYPOINTS ----------
    flag: `
        <path d="M5 4 V21" />
        <path d="M5 4 H18 L15 8 L18 12 H5" />
    `,
    gate: `
        <path d="M3 21 V7 H21 V21" />
        <path d="M3 21 H21" />
        <path d="M12 7 V21" />
        <circle cx="9" cy="14" r="0.8" fill="currentColor" stroke="none" />
    `,
    home: `
        <path d="M3 11 L12 3 L21 11" />
        <path d="M5 10 V20 H19 V10" />
        <path d="M10 20 V14 H14 V20" />
    `,
    tent: `
        <path d="M12 3 L3 21 H21 Z" />
        <path d="M12 3 V21" />
        <path d="M8 21 L12 14 L16 21" />
    `,
    summit: `
        <path d="M2 20 L9 8 L13 14 L17 6 L22 20 Z" />
        <path d="M9 8 L11 11" />
    `,
    junction: `
        <path d="M12 3 V21" />
        <path d="M12 11 L18 5" />
        <path d="M14 5 H18 V9" />
        <path d="M12 13 L6 19" />
        <path d="M10 19 H6 V15" />
    `,
    pin: `
        <path d="M12 3 A6 6 0 0 1 18 9 C18 14 12 21 12 21 C12 21 6 14 6 9 A6 6 0 0 1 12 3 Z" />
        <circle cx="12" cy="9" r="2" fill="currentColor" stroke="none" />
    `,
    water: `
        <path d="M12 3 C12 3 5 11 5 15 A7 7 0 0 0 19 15 C19 11 12 3 12 3 Z" />
    `,
    circleDot: `
        <circle cx="12" cy="12" r="9" />
        <circle cx="12" cy="12" r="3" fill="currentColor" stroke="none" />
    `,

    // ---------- MISC ----------
    chartLine: `
        <path d="M3 20 H21" />
        <path d="M5 16 L9 10 L13 14 L19 5" />
    `,
    arrowRight: `
        <path d="M5 12 H19" />
        <path d="M13 5 L19 12 L13 19" />
    `,
    arrowLeft: `
        <path d="M19 12 H5" />
        <path d="M11 5 L5 12 L11 19" />
    `,
    eye: `
        <path d="M2 12 C5 6 8 4 12 4 C16 4 19 6 22 12 C19 18 16 20 12 20 C8 20 5 18 2 12 Z" />
        <circle cx="12" cy="12" r="3" />
    `,
    eyeOff: `
        <path d="M3 3 L21 21" />
        <path d="M10.6 6.2 C11.1 6.07 11.55 6 12 6 C16 6 19 8 22 12 C21 13.3 20.3 14.4 19.4 15.4" />
        <path d="M6.7 7.7 C4.5 9 3 10.6 2 12 C5 17.5 8.4 19 12 19 C13.5 19 14.9 18.6 16.2 17.9" />
        <path d="M9.5 9.5 A3 3 0 0 0 14.5 14.5" />
    `,
    shield: `
        <path d="M12 3 L20 6 V12 C20 17 16 21 12 22 C8 21 4 17 4 12 V6 Z" />
        <path d="M9 12 L11 14 L15 10" />
    `,

    // ---------- BATTERY ----------
    // Body battery + tip kanan + level fill (dynamic via render context).
    // Untuk simplicity kita pakai 4 varian fixed level — render function
    // batteryIcon(percent) di dashboard.js akan pilih varian sesuai threshold.
    batteryFull: `
        <rect x="2" y="8" width="17" height="10" rx="1.5" />
        <rect x="20" y="11" width="2" height="4" fill="currentColor" stroke="none" />
        <rect x="4" y="10" width="13" height="6" fill="currentColor" stroke="none" />
    `,
    batteryHigh: `
        <rect x="2" y="8" width="17" height="10" rx="1.5" />
        <rect x="20" y="11" width="2" height="4" fill="currentColor" stroke="none" />
        <rect x="4" y="10" width="9" height="6" fill="currentColor" stroke="none" />
    `,
    batteryMid: `
        <rect x="2" y="8" width="17" height="10" rx="1.5" />
        <rect x="20" y="11" width="2" height="4" fill="currentColor" stroke="none" />
        <rect x="4" y="10" width="6" height="6" fill="currentColor" stroke="none" />
    `,
    batteryLow: `
        <rect x="2" y="8" width="17" height="10" rx="1.5" />
        <rect x="20" y="11" width="2" height="4" fill="currentColor" stroke="none" />
        <rect x="4" y="10" width="3" height="6" fill="currentColor" stroke="none" />
    `,
    batteryEmpty: `
        <rect x="2" y="8" width="17" height="10" rx="1.5" />
        <rect x="20" y="11" width="2" height="4" fill="currentColor" stroke="none" />
        <path d="M9 11 L13 15 M13 11 L9 15" stroke-width="2" />
    `,
    batteryUnknown: `
        <rect x="2" y="8" width="17" height="10" rx="1.5" />
        <rect x="20" y="11" width="2" height="4" fill="currentColor" stroke="none" />
        <path d="M10 11 A 1.5 1.5 0 0 1 13 11 C 13 13 11 13 11 14" stroke-width="2"/>
        <circle cx="11" cy="16" r="0.6" fill="currentColor" stroke="none"/>
    `,
};

/**
 * Render icon sebagai HTML string. Cocok untuk innerHTML / template
 * literal. Default size 18px (lebih kecil dari emoji 20px tapi lebih
 * tegas berkat stroke 2.5).
 *
 * @param {string} name  Nama icon dari ICON_PATHS.
 * @param {number} [size=18]  Pixel size (lebar = tinggi).
 * @param {string} [extraClass=""]  Class tambahan untuk styling.
 * @returns {string} HTML SVG string.
 */
function ICON(name, size, extraClass) {
    const px = size || 18;
    const cls = extraClass || "";
    const inner = ICON_PATHS[name];
    if (!inner) {
        // Fallback ke kotak kosong + warning di console — render tetap
        // tidak crash kalau ada nama typo.
        if (typeof console !== "undefined" && console.warn) {
            console.warn(`[ALTIVEX] icon '${name}' tidak ditemukan`);
        }
        return `<svg width="${px}" height="${px}" viewBox="0 0 24 24" class="icon ${cls}" aria-hidden="true"><rect x="2" y="2" width="20" height="20" fill="none" stroke="currentColor" stroke-width="2.5"/></svg>`;
    }
    return (
        `<svg width="${px}" height="${px}" viewBox="0 0 24 24" ` +
        `fill="none" stroke="currentColor" stroke-width="2.5" ` +
        `stroke-linecap="round" stroke-linejoin="round" ` +
        `class="icon ${cls}" aria-hidden="true">${inner}</svg>`
    );
}

/**
 * Render icon sebagai DOM Node. Cocok untuk createElement-based code
 * (mis. Leaflet divIcon, dynamic insertion).
 */
function iconNode(name, size, extraClass) {
    const div = document.createElement("div");
    div.innerHTML = ICON(name, size, extraClass).trim();
    return div.firstChild;
}

// Expose ke global supaya dashboard.js (non-module) bisa pakai langsung.
window.ICON = ICON;
window.iconNode = iconNode;
window.ICON_PATHS = ICON_PATHS;
