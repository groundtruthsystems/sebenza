import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

const backendPort = process.env.PORT || "5111";
const backendUrl = `http://localhost:${backendPort}`;
const backendWs = `ws://localhost:${backendPort}`;
const port = parseInt(process.env.FRONTEND_PORT || "5112");

export default defineConfig({
  plugins: [react(), tailwindcss()],
  build: {
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes("node_modules/@xterm/")) {
            return "vendor-xterm";
          }
        },
      },
    },
  },
  server: {
    host: "0.0.0.0",
    allowedHosts: ["diego-devbox"],
    port,
    proxy: {
      // Match both hub routes (`/api`, `/ws`) and per-project prefixed routes
      // (`/<prefix>/api`, `/<prefix>/ws`) — the frontend always runs under a
      // project prefix, so unprefixed-only rules would serve index.html for
      // every data call. Regex keys (leading `^`) are matched against the path.
      "^(/[^/]+)?/api": backendUrl,
      "^(/[^/]+)?/ws": {
        target: backendWs,
        ws: true,
      },
    },
  },
  preview: {
    host: "0.0.0.0",
    port: 4173,
    proxy: {
      "^(/[^/]+)?/api": backendUrl,
      "^(/[^/]+)?/ws": {
        target: backendWs,
        ws: true,
      },
    },
  },
});
