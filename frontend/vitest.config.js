import { defineConfig } from "vitest/config";

// Konfigurasi minimal untuk test harness frontend ALTIVEX.
// Lingkungan jsdom diperlukan agar `document`, `Notification`, dst.
// tersedia saat test berjalan tanpa browser sebenarnya.
export default defineConfig({
    test: {
        environment: "jsdom",
        include: ["tests/**/*.spec.js"],
        // Property-based tests bisa lambat saat ada banyak run; beri
        // ruang lebih dari default 5s.
        testTimeout: 20000,
    },
});
