import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import tailwindcss from "@tailwindcss/vite";
import { resolve } from "node:path";

// Tauri 多窗口：每个窗口一个 HTML 入口（07 §11）
export default defineConfig({
  plugins: [vue(), tailwindcss()],
  resolve: {
    alias: { "@": resolve(__dirname, "src") },
  },
  build: {
    rollupOptions: {
      input: {
        hud: resolve(__dirname, "src/windows/hud/index.html"),
        assistant: resolve(__dirname, "src/windows/assistant/index.html"),
        settings: resolve(__dirname, "src/windows/settings/index.html"),
        onboarding: resolve(__dirname, "src/windows/onboarding/index.html"),
        home: resolve(__dirname, "src/windows/home/index.html"),
      },
    },
  },
  // Tauri 开发约定
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
});
