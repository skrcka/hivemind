import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri reads this port from tauri.conf.json → build.devUrl.
// Keep them in sync if you change it.
const PORT = 1420;

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: PORT,
    strictPort: true,
    host: "127.0.0.1",
    // Tauri watches the Rust files itself; we only need vite to watch JS/TS/CSS.
    watch: { ignored: ["**/src-tauri/**", "**/target/**"] },
  },
  build: {
    target: "es2021",
    minify: "esbuild",
    sourcemap: true,
    outDir: "dist",
    emptyOutDir: true,
  },
});
