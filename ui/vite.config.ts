import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The daemon is the only backend. In dev, Vite proxies /api to it instead of
// serving its own copy of anything: the API is the contract, and the dev server
// must not be a place where the app behaves differently than in production.
const API = process.env.LOCALVOX_API_BIND ?? "127.0.0.1:3017";

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      "/api": { target: `http://${API}`, changeOrigin: false },
      "/manifest.webmanifest": { target: `http://${API}`, changeOrigin: false },
    },
  },
  build: {
    // Embedded into the binary by localvox-light-api. Hashed asset names are what
    // makes `Cache-Control: immutable` on them honest.
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: false,
  },
});
