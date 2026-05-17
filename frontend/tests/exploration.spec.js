/**
 * Exploration test (frontend) — Task 1 + Task 3.14 spec
 * altivex-critical-fixes.
 *
 * ## Status
 *
 * Task 1 (initial run, kode F): GAGAL → bukti bahwa Bug F1 (Stored XSS via
 * `innerHTML`) dan Bug F3 (race WS+polling yang menghapus flag `notified`)
 * memang ada di kode F. Counter-example yang tercatat:
 *   - F1: `<img src=x onerror=alert(1)>` → `<img>` muncul di DOM.
 *   - F3: `[ws, poll, ws]` → notifyCount = 2 (≥1 spam).
 *
 * Task 3.14 (re-run setelah fix): replika in-test DI-PORT mengikuti F'
 * (post-fix shape dari task 3.9 escapeHtml + task 3.11 notifiedDevices Map).
 * Property assertion-nya tetap sama; counter-example seed historis tetap
 * dipakai sebagai `examples`, hanya sekarang menghasilkan PASS karena
 * implementasi F' menutup kedua celah. Deviasi dari "JANGAN tulis test
 * baru" didokumentasikan di tasks.md (jawaban user pada task 3.14).
 *
 * ## Validates
 *
 * - **Validates: Requirements 1.11, 2.11** (F1 — Stored XSS)
 * - **Validates: Requirements 1.13, 2.13** (F3 — Race notifikasi)
 */

import { describe, it, beforeEach } from "vitest";
import fc from "fast-check";

// ---------------------------------------------------------------------------
// Replika F' (post-fix) dari `altivex_backend/frontend/index.html`.
//
// Helper ini DICOPY VERBATIM dari produksi supaya test berbicara terhadap
// kontrak yang sama. Kalau produksi diubah, salinan di sini WAJIB ikut
// di-update; kalau tidak, drift akan menghasilkan counter-example yang
// menyesatkan.
// ---------------------------------------------------------------------------

/**
 * Helper escapeHtml dari index.html (Task 3.9 — Bug F1). Mengubah karakter
 * spesial HTML jadi entity. `null`/`undefined` → "". Untuk teks polos
 * (alfanumerik + spasi), output identik input → preservation 3.8 lulus.
 */
function escapeHtml(v) {
    return String(v ?? "")
        .replaceAll("&", "&amp;")
        .replaceAll("<", "&lt;")
        .replaceAll(">", "&gt;")
        .replaceAll('"', "&quot;")
        .replaceAll("'", "&#39;");
}

/**
 * notifiedDevices Map dari index.html (Task 3.11 — Bug F3). Module-scope
 * di test file, di-clear di `beforeEach`/`buildScenario` agar test cases
 * tidak bocor.
 */
const notifiedDevices = new Map();
function setNotified(id, val) {
    notifiedDevices.set(id, val === true);
}
function isNotified(id) {
    return notifiedDevices.get(id) === true;
}

/**
 * Replika `renderHistoryTable` versi F' (post-fix). SEMUA penyisipan
 * data user-controlled lewat `escapeHtml`. Mirror produksi yang
 * me-render via template literal + `tbody.innerHTML`.
 */
function renderHistoryTable_FPrime(data, tbody) {
    tbody.innerHTML = data
        .map(
            (p) => `
                <tr>
                    <td>${escapeHtml(p.nama_pendaki)}</td>
                    <td><span class="neo-badge">${escapeHtml(p.id_perangkat)}</span></td>
                    <td>${escapeHtml(p.telepon_darurat)}</td>
                    <td>
                        <span class="neo-badge">${escapeHtml(p.status)}</span>
                    </td>
                    <td>${escapeHtml(new Date(p.tanggal_naik).toLocaleString("id-ID"))}</td>
                    <td>
                        <button class="neo-btn neo-btn-sm" data-action="edit" data-id="${escapeHtml(String(p.id ?? ""))}">✏️</button>
                    </td>
                </tr>
            `
        )
        .join("");
}

/**
 * Replika `_renderHikerCards` versi F' (post-fix). Flag notified disimpan
 * di `notifiedDevices` Map terpisah, BUKAN sebagai property pada objek
 * `latestDataPerDevice[id]`. WS dan polling boleh overwrite objek tersebut
 * sesukanya — flag tidak terdampak.
 */
function renderHikerCards_FPrime(
    latestDataPerDevice,
    registeredHikers,
    isOutside,
    sendNotification
) {
    for (const id in latestDataPerDevice) {
        const data = latestDataPerDevice[id];
        const hiker = registeredHikers[id];
        if (!hiker) continue;

        if (isOutside(data)) {
            if (!isNotified(id)) {
                sendNotification();
                setNotified(id, true);
            }
        } else {
            setNotified(id, false);
        }
    }
}

/**
 * Replika handler WebSocket (overwrite wholesale) — TIDAK diubah oleh
 * task 3.11. Yang berubah hanya tempat penyimpanan flag `notified`.
 */
function wsPush_FPrime(latestDataPerDevice, data) {
    latestDataPerDevice[data.id_perangkat] = data;
}

/**
 * Replika polling fetch (overwrite wholesale) — sama, tidak diubah.
 */
function pollFetch_FPrime(latestDataPerDevice, sensorList) {
    sensorList.forEach((d) => {
        latestDataPerDevice[d.id_perangkat] = d;
    });
}

// ---------------------------------------------------------------------------
// PBT 1: F1 — Stored XSS via innerHTML (post-fix harus PASS)
// ---------------------------------------------------------------------------
//
// Property (sesuai requirements 2.11):
//   FORALL s ∈ String:
//     renderHistoryTable_FPrime([{...defaults, nama_pendaki: s}]) ⇒
//       tbody.querySelectorAll("img,script,svg,iframe,object,embed").length === 0
//
// Pada kode F, GAGAL untuk `<img src=x onerror=...>`. Pada kode F'
// (escapeHtml di task 3.9) seluruh `<` ter-encode `&lt;` sehingga browser
// tidak mem-parse-nya sebagai elemen → length === 0 untuk SEMUA input.

describe("Exploration F1 — Stored XSS via innerHTML", () => {
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
     * Validates: Requirements 1.11, 2.11
     *
     * Property F1 — `renderHistoryTable` (versi F') SHALL tidak menghasilkan
     * elemen eksekusi (img/script/svg/iframe/object/embed) untuk SEMUA
     * nilai `nama_pendaki`, termasuk string yang mengandung markup HTML.
     */
    it("renderHistoryTable tidak boleh menyuntikkan elemen eksekusi (PBT)", () => {
        fc.assert(
            fc.property(fc.string({ minLength: 1, maxLength: 200 }), (s) => {
                const row = {
                    id: 1,
                    nama_pendaki: s,
                    id_perangkat: "ALAT-001",
                    telepon_darurat: "0800",
                    status: "Mendaki",
                    tanggal_naik: "2026-01-01T00:00:00Z",
                };
                renderHistoryTable_FPrime([row], tbody);
                const dangerous = tbody.querySelectorAll(
                    "img,script,svg,iframe,object,embed"
                );
                return dangerous.length === 0;
            }),
            {
                // Counter-example historis dari task 1 (kode F): pada F'
                // sekarang harus tetap menghasilkan length === 0.
                examples: [["<img src=x onerror=alert(1)>"]],
                numRuns: 8,
            }
        );
    });
});

// ---------------------------------------------------------------------------
// PBT 2: F3 — Race WS + Polling melepas flag `notified` (post-fix harus PASS)
// ---------------------------------------------------------------------------
//
// Property (sesuai requirements 2.13):
//   FORALL urutan event ∈ {wsPush, pollFetch}* dengan id tetap di luar buffer:
//     setelah seluruh event diproses dan `_renderHikerCards` dijalankan,
//       sendNotification dipanggil ≤ 1 kali.
//
// Pada kode F, GAGAL untuk `[ws, poll, ws]` (overwrite objek = flag
// hilang). Pada kode F' (notifiedDevices Map terpisah di task 3.11),
// flag bertahan walau objek device di-overwrite → notifyCount ≤ 1.

describe("Exploration F3 — Race WS + Polling spam notifikasi", () => {
    const HIKER_ID = "ALAT-001";

    function buildScenario() {
        // Reset Map module-scope agar test cases tidak bocor satu sama
        // lain (PBT shrinker akan men-rerun banyak kali).
        notifiedDevices.clear();

        const latestDataPerDevice = {};
        const registeredHikers = {
            [HIKER_ID]: {
                nama_pendaki: "Test Hiker",
                telepon_darurat: "0800",
            },
        };
        let notifyCount = 0;
        const sendNotification = () => {
            notifyCount += 1;
        };
        const isOutside = () => true;

        return {
            latestDataPerDevice,
            registeredHikers,
            getNotifyCount: () => notifyCount,
            sendNotification,
            isOutside,
        };
    }

    /**
     * Validates: Requirements 1.13, 2.13
     *
     * Property F3 — selama pendaki tetap di luar buffer, urutan event
     * apa pun dari {ws, poll} SHALL menghasilkan jumlah notifikasi ≤ 1.
     */
    it("sendNotification SHALL dipanggil ≤ 1 untuk urutan event apa pun (PBT)", () => {
        fc.assert(
            fc.property(
                fc.array(fc.constantFrom("ws", "poll"), {
                    minLength: 1,
                    maxLength: 10,
                }),
                (events) => {
                    const scn = buildScenario();
                    const outsidePos = {
                        id_perangkat: HIKER_ID,
                        latitude: -6.7,
                        longitude: 106.95,
                    };

                    for (const ev of events) {
                        if (ev === "ws") {
                            wsPush_FPrime(scn.latestDataPerDevice, { ...outsidePos });
                        } else {
                            pollFetch_FPrime(scn.latestDataPerDevice, [
                                { ...outsidePos },
                            ]);
                        }
                        renderHikerCards_FPrime(
                            scn.latestDataPerDevice,
                            scn.registeredHikers,
                            scn.isOutside,
                            scn.sendNotification
                        );
                    }

                    return scn.getNotifyCount() <= 1;
                }
            ),
            {
                // Counter-example historis dari task 1 (kode F).
                examples: [[["ws", "poll", "ws"]]],
                numRuns: 8,
            }
        );
    });
});
