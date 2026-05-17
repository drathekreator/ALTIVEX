/**
 * Preservation tests (frontend) untuk ALTIVEX baseline behavior —
 * Task 2 spec altivex-critical-fixes.
 *
 * ## Tujuan
 *
 * Test ini WAJIB LULUS pada kode F (versi `index.html` saat ini,
 * sebelum fix). Kelulusan-nya mendokumentasikan baseline yang TIDAK
 * BOLEH regress setelah paket fix di task 3 di-merge.
 *
 * Methodologi: observation-first. Kita observasi behavior kode F
 * lalu meng-encode-nya sebagai property/assertion. Karena
 * `index.html` adalah static file dengan logika di dalam tag
 * `<script>`, kita tidak bisa import langsung — sebagai gantinya
 * kita REPLIKASI fungsi yang relevan apa adanya (sama strategi
 * dengan `exploration.spec.js`).
 *
 * ## Cakupan (mengikuti tasks.md task 2)
 *
 * - **3.8**: `renderHistoryTable([{...defaults, nama_pendaki: name}])`
 *   untuk `name in alphanum_with_spaces` menghasilkan
 *   `tbody.textContent` yang mengandung `name` apa adanya (tanpa
 *   entity HTML).
 * - **3.10**: WS push pendaki yang berada di dalam buffer geofence
 *   → `sendNotification` TIDAK terpanggil.
 * - **3.12**: Search kosong + filter "Semua" → `renderHistoryTable`
 *   dipanggil dengan `historyData` lengkap (length sama).
 * - **3.13**: Untuk data polos (regex `/^[A-Za-z0-9 ]+$/`),
 *   `exportCSV` menghasilkan blob yang ketika di-parse kembali
 *   (split sederhana) menghasilkan jumlah kolom = headers.
 *
 * ## Validates
 *
 * - **Validates: Requirements 3.8** (Render nama polos identik)
 * - **Validates: Requirements 3.10** (Pendaki di buffer = no notif)
 * - **Validates: Requirements 3.12** (Search+filter default = full data)
 * - **Validates: Requirements 3.13** (CSV layout polos identik)
 */

import { describe, it, expect, beforeEach, vi } from "vitest";
import fc from "fast-check";

// ---------------------------------------------------------------------------
// Replika logika produksi dari `altivex_backend/frontend/index.html`
// (versi F — sebelum fix). Kita salin verbatim agar test berbicara
// tepat terhadap perilaku produksi yang berlaku saat ini.
// ---------------------------------------------------------------------------

/**
 * Replika `renderHistoryTable` dari index.html line ≈ 730. Versi F
 * memakai template literal + `tbody.innerHTML = ...`. Untuk nama
 * polos (tanpa HTML), perilaku yang harus dipertahankan adalah:
 * teks nama tampil identik di `tbody.textContent`.
 */
function renderHistoryTable_F(data, tbody) {
    tbody.innerHTML = data
        .map(
            (p) => `
                <tr>
                    <td>${p.nama_pendaki}</td>
                    <td><span class="neo-badge">${p.id_perangkat}</span></td>
                    <td>${p.telepon_darurat}</td>
                    <td>
                        <span class="neo-badge">${p.status}</span>
                    </td>
                    <td>${new Date(p.tanggal_naik).toLocaleString("id-ID")}</td>
                    <td>
                        <button class="neo-btn neo-btn-sm">✏️</button>
                    </td>
                </tr>
            `
        )
        .join("");
}

/**
 * Replika `_renderHikerCards` (subset notifikasi) dari index.html
 * line ≈ 847. Saat pendaki BERADA DI DALAM buffer (`isInside === true`),
 * branch alert tidak dieksekusi → `sendNotification` tidak dipanggil.
 */
function renderHikerCards_F(
    latestDataPerDevice,
    registeredHikers,
    isInsideFn,
    sendNotification
) {
    for (const id in latestDataPerDevice) {
        const data = latestDataPerDevice[id];
        const hiker = registeredHikers[id];
        if (!hiker) continue;

        const isInside = isInsideFn(data);
        if (!isInside) {
            if (!latestDataPerDevice[id].notified) {
                sendNotification();
                latestDataPerDevice[id].notified = true;
            }
        } else {
            latestDataPerDevice[id].notified = false;
        }
    }
}

/**
 * Replika `searchPendaki` + `filterHistory` dari index.html line
 * ≈ 991–1003. Catatan: di kode F keduanya independen (tidak
 * compose) — yang kita uji di preservation 3.12 hanyalah jalur
 * default (search kosong + filter "Semua"), yang DI BOTH branch
 * memang menghasilkan render dengan historyData lengkap.
 */
function searchPendaki_F(historyData, qInput, renderFn) {
    const q = qInput.toLowerCase();
    const filtered = historyData.filter((p) =>
        p.nama_pendaki.toLowerCase().includes(q)
    );
    renderFn(filtered);
}

function filterHistory_F(historyData, filterValue, renderFn) {
    if (filterValue === "Semua") {
        renderFn(historyData);
        return;
    }
    const filtered = historyData.filter((p) => p.status === filterValue);
    renderFn(filtered);
}

/**
 * Replika `exportCSV` dari index.html line ≈ 802. Kode F join
 * fields dengan koma tanpa escape, memakai `data:text/csv;...` URI
 * → kita kembalikan string CSV (bukan blob) agar mudah di-assert.
 * Untuk data polos `/^[A-Za-z0-9 ]+$/` (tanpa koma/quote/newline),
 * format ini SHALL menghasilkan jumlah kolom = headers.
 */
function exportCSV_F(historyData) {
    const headers = ["Nama", "ID Alat", "Telepon", "Status", "Waktu Naik"];
    const rows = historyData.map((p) => [
        p.nama_pendaki,
        p.id_perangkat,
        p.telepon_darurat,
        p.status,
        p.tanggal_naik,
    ]);
    return headers.join(",") + "\n" + rows.map((e) => e.join(",")).join("\n");
}

// ---------------------------------------------------------------------------
// PBT 3.8 — renderHistoryTable nama polos tampil apa adanya
// ---------------------------------------------------------------------------
//
// Property (clause 3.8):
//   FORALL name ∈ /^[A-Za-z0-9 ]+$/:
//     setelah renderHistoryTable([{...defaults, nama_pendaki: name}]),
//       tbody.textContent SHALL mengandung name persis (tanpa entity).
//
// Pada kode F, property ini lulus karena nama polos tidak terkena
// HTML escaping (innerHTML mem-parse `name` sebagai teks). Pada kode
// F' (escapeHtml di task 3.9), property tetap lulus karena
// escapeHtml hanya mengubah `& < > " '` — yang tidak muncul di
// regex polos.

describe("Preservation 3.8 — renderHistoryTable nama polos identik", () => {
    /** @type {HTMLTableSectionElement} */
    let tbody;

    beforeEach(() => {
        document.body.innerHTML = `
            <table>
                <tbody id="history-table-body"></tbody>
            </table>
        `;
        tbody = /** @type {HTMLTableSectionElement} */ (
            document.getElementById("history-table-body")
        );
    });

    /**
     * Validates: Requirements 3.8
     *
     * Untuk semua nama pendaki polos (alphanumeric + spasi),
     * `tbody.textContent` SHALL mengandung nama persis seperti
     * input — tidak boleh ada HTML entity (`&amp;`, `&lt;`, dll.)
     * yang muncul.
     */
    it("nama polos render identik di tbody.textContent (PBT)", () => {
        fc.assert(
            fc.property(
                fc
                    .stringMatching(/^[A-Za-z0-9 ]{1,40}$/)
                    .filter((s) => s.trim().length > 0),
                (name) => {
                    const row = {
                        id: 1,
                        nama_pendaki: name,
                        id_perangkat: "ALAT-001",
                        telepon_darurat: "081234567890",
                        status: "Mendaki",
                        tanggal_naik: "2026-01-01T08:00:00Z",
                    };
                    renderHistoryTable_F([row], tbody);

                    // (a) textContent mengandung nama apa adanya
                    if (!tbody.textContent.includes(name)) return false;

                    // (b) Tidak ada entity yang muncul untuk
                    //     karakter polos (sanity: textContent
                    //     tidak boleh berisi `&amp;` atau `&lt;`
                    //     untuk input yang tidak punya `&` atau `<`).
                    if (
                        tbody.textContent.includes("&amp;") &&
                        !name.includes("&amp;")
                    )
                        return false;
                    if (
                        tbody.textContent.includes("&lt;") &&
                        !name.includes("&lt;")
                    )
                        return false;

                    return true;
                }
            ),
            { numRuns: 8 }
        );
    });
});

// ---------------------------------------------------------------------------
// PBT 3.10 — Pendaki di dalam buffer → no sendNotification
// ---------------------------------------------------------------------------
//
// Property (clause 3.10):
//   FORALL pendaki state dengan posisi di dalam buffer:
//     setelah _renderHikerCards dijalankan (pada beragam urutan WS
//     push), sendNotification TIDAK terpanggil.
//
// Pada kode F, ini lulus karena branch `if (!isInside)` adalah
// satu-satunya jalur yang memicu sendNotification. Setelah fix
// (notifiedDevices Map di 3.11), tetap lulus.

describe("Preservation 3.10 — Pendaki di dalam buffer tidak memicu notifikasi", () => {
    const HIKER_ID = "ALAT-001";

    function buildScenario() {
        const latestDataPerDevice = {};
        const registeredHikers = {
            [HIKER_ID]: {
                nama_pendaki: "Aman Polos",
                telepon_darurat: "081200000000",
            },
        };
        const sendNotification = vi.fn();
        const isInsideAlways = () => true; // posisi di dalam buffer
        return {
            latestDataPerDevice,
            registeredHikers,
            sendNotification,
            isInsideAlways,
        };
    }

    /**
     * Validates: Requirements 3.10
     *
     * Selama pendaki tetap di dalam buffer, urutan WS push apa pun
     * SHALL menghasilkan 0 panggilan `sendNotification`.
     */
    it("WS push pendaki dalam buffer → sendNotification tidak terpanggil (PBT)", () => {
        fc.assert(
            fc.property(
                fc.array(
                    fc.record({
                        latitude: fc.double({
                            min: -90,
                            max: 90,
                            noNaN: true,
                        }),
                        longitude: fc.double({
                            min: -180,
                            max: 180,
                            noNaN: true,
                        }),
                    }),
                    { minLength: 1, maxLength: 10 }
                ),
                (pushes) => {
                    const scn = buildScenario();
                    for (const pos of pushes) {
                        // Mirror: ws.onmessage handler di
                        // index.html line ≈ 1009.
                        scn.latestDataPerDevice[HIKER_ID] = {
                            id_perangkat: HIKER_ID,
                            latitude: pos.latitude,
                            longitude: pos.longitude,
                        };
                        renderHikerCards_F(
                            scn.latestDataPerDevice,
                            scn.registeredHikers,
                            scn.isInsideAlways,
                            scn.sendNotification
                        );
                    }
                    return scn.sendNotification.mock.calls.length === 0;
                }
            ),
            { numRuns: 8 }
        );
    });
});

// ---------------------------------------------------------------------------
// PBT 3.12 — Search kosong + filter "Semua" → render data lengkap
// ---------------------------------------------------------------------------
//
// Property (clause 3.12):
//   FORALL historyData (list of pendaki):
//     - searchPendaki_F(historyData, "", render) ⇒
//         render dipanggil dengan length === historyData.length
//     - filterHistory_F(historyData, "Semua", render) ⇒
//         render dipanggil dengan length === historyData.length
//
// Pada kode F, ini lulus karena jalur default (q="" → includes
// match semua, filter="Semua" → return historyData) tidak
// memfilter apa pun.

describe("Preservation 3.12 — Search kosong + filter Semua → data lengkap", () => {
    /**
     * Validates: Requirements 3.12
     *
     * Search input kosong dengan filter default "Semua" SHALL
     * memanggil `renderHistoryTable` dengan `historyData` lengkap
     * (length sama dengan input).
     */
    it("default search+filter render full historyData (PBT)", () => {
        fc.assert(
            fc.property(
                fc.array(
                    fc.record({
                        id: fc.integer({ min: 1, max: 1000 }),
                        nama_pendaki: fc.stringMatching(/^[A-Za-z ]{1,30}$/),
                        id_perangkat: fc.stringMatching(/^[A-Z0-9-]{1,16}$/),
                        telepon_darurat: fc.stringMatching(/^[0-9]{8,14}$/),
                        status: fc.constantFrom("Mendaki", "Sudah Turun"),
                        tanggal_naik: fc.constant("2026-01-01T08:00:00Z"),
                    }),
                    { minLength: 0, maxLength: 30 }
                ),
                (historyData) => {
                    // (a) search kosong → render full
                    let captured1 = null;
                    searchPendaki_F(historyData, "", (d) => {
                        captured1 = d;
                    });
                    if (!captured1 || captured1.length !== historyData.length)
                        return false;

                    // (b) filter "Semua" → render full
                    let captured2 = null;
                    filterHistory_F(historyData, "Semua", (d) => {
                        captured2 = d;
                    });
                    if (!captured2 || captured2.length !== historyData.length)
                        return false;

                    return true;
                }
            ),
            { numRuns: 8 }
        );
    });
});

// ---------------------------------------------------------------------------
// PBT 3.13 — exportCSV data polos kolom rapi
// ---------------------------------------------------------------------------
//
// Property (clause 3.13):
//   FORALL row dengan field-field cocok /^[A-Za-z0-9 ]+$/:
//     parse-back exportCSV via split sederhana ⇒
//       semua baris (header + data) memiliki jumlah kolom = headers.length
//
// Pada kode F, untuk data polos (tanpa `,`, `"`, atau newline),
// `join(",")` menghasilkan baris dengan jumlah kolom yang konsisten.
// Setelah fix (csvField di 3.13), output untuk input polos tetap
// identik karena `csvField` hanya quote saat ada karakter spesial.

describe("Preservation 3.13 — exportCSV data polos punya kolom konsisten", () => {
    /**
     * Validates: Requirements 3.13
     *
     * Untuk historyData yang seluruh field-nya polos
     * (`/^[A-Za-z0-9 ]+$/` — tidak ada koma, kuotasi, atau newline),
     * blob CSV yang di-parse kembali via split sederhana SHALL
     * menghasilkan jumlah kolom yang sama dengan headers untuk
     * setiap baris.
     */
    it("CSV polos: setiap baris punya kolom = headers (PBT)", () => {
        const HEADERS_LEN = 5;

        const polosArb = fc.stringMatching(/^[A-Za-z0-9 ]{1,20}$/);

        fc.assert(
            fc.property(
                fc.array(
                    fc.record({
                        nama_pendaki: polosArb,
                        id_perangkat: polosArb,
                        telepon_darurat: polosArb,
                        status: fc.constantFrom("Mendaki", "Sudah Turun"),
                        tanggal_naik: polosArb,
                    }),
                    { minLength: 1, maxLength: 10 }
                ),
                (rows) => {
                    const csv = exportCSV_F(rows);
                    const lines = csv.split("\n");
                    // header + N rows
                    if (lines.length !== rows.length + 1) return false;
                    // setiap baris memiliki HEADERS_LEN kolom
                    return lines.every(
                        (line) => line.split(",").length === HEADERS_LEN
                    );
                }
            ),
            { numRuns: 8 }
        );
    });
});

// ---------------------------------------------------------------------------
// Sanity tests — pastikan harness sendiri tidak buggy
// ---------------------------------------------------------------------------

describe("Preservation sanity (deterministik)", () => {
    /** @type {HTMLTableSectionElement} */
    let tbody;

    beforeEach(() => {
        document.body.innerHTML = `
            <table>
                <tbody id="history-table-body"></tbody>
            </table>
        `;
        tbody = /** @type {HTMLTableSectionElement} */ (
            document.getElementById("history-table-body")
        );
    });

    it("renderHistoryTable nama 'Aman Polos' tampil utuh", () => {
        renderHistoryTable_F(
            [
                {
                    id: 1,
                    nama_pendaki: "Aman Polos",
                    id_perangkat: "ALAT-001",
                    telepon_darurat: "081234567890",
                    status: "Mendaki",
                    tanggal_naik: "2026-01-01T08:00:00Z",
                },
            ],
            tbody
        );
        expect(tbody.textContent).toContain("Aman Polos");
    });

    it("exportCSV dengan satu baris polos → 2 lines, 5 kolom each", () => {
        const csv = exportCSV_F([
            {
                nama_pendaki: "Aman",
                id_perangkat: "ALAT 001",
                telepon_darurat: "081200000000",
                status: "Mendaki",
                tanggal_naik: "2026 01 01",
            },
        ]);
        const lines = csv.split("\n");
        expect(lines.length).toBe(2);
        expect(lines[0].split(",").length).toBe(5);
        expect(lines[1].split(",").length).toBe(5);
    });
});
