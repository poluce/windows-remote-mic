import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// dev server 默认只听本机回环：这台应用只跑 Windows 桌面，不需要被同网段访问。
// 早期照 Tauri 模板写成 0.0.0.0，会把 1420 暴露给局域网（而 dev server 正是
// 多条 [vite] 安全通告的攻击面）。要在手机上调试时显式设 TAURI_DEV_HOST 再打开。
const devHost = host || "127.0.0.1";

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: devHost,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`, `target`, and `crates`
      ignored: ["**/src-tauri/**", "**/target/**", "**/crates/**"],
    },
  },
}));
