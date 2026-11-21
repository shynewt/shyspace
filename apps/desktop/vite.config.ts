import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  // `tauri dev` and `tauri build` set TAURI_ENV_DEBUG and friends for the frontend.
  envPrefix: ["VITE_", "TAURI_ENV_"],
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**"] } },
});
