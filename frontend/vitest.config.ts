import { defineConfig, mergeConfig } from "vitest/config";
import viteConfig from "./vite.config";

export default mergeConfig(viteConfig, defineConfig({
  resolve: {
    conditions: ["browser"],
  },
  test: {
    globals: true,
    environment: "happy-dom",
    include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
    setupFiles: ["src/test-setup.ts"],
    // MobileChatSurface uses real-time-advancing fake timers; give polling
    // assertions headroom so parallel CPU contention doesn't trip the timeout.
    testTimeout: 15000,
    coverage: {
      provider: "v8",
      include: ["src/**/*.ts", "src/**/*.tsx"],
      exclude: ["src/test-setup.ts", "src/vite-env.d.ts", "src/**/__tests__/**"],
    },
  },
}));
