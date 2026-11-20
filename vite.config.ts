import process from "node:process";
import solid from "vite-plugin-solid";
import { defineConfig } from "vitest/config";

const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig({
  plugins: [solid()],

  // Options tailored for `tauri dev` and `tauri build`:
  // 1. don't hide Rust errors
  clearScreen: false,
  // 2. Tauri expects a fixed port; fail if it's taken
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    // 3. don't watch the Rust side
    watch: { ignored: ["**/src-tauri/**"] },
  },

  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
    setupFiles: ["./src/test/setup.ts"],
  },
});
