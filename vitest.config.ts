import { defineConfig } from "vitest/config";

// Unit tests for the frontend's pure logic (no DOM, no Tauri): `npm run test:web`.
// The Rust side has its own suite: `npm test`.
export default defineConfig({
  test: {
    include: ["src/**/*.test.ts"],
    environment: "node",
  },
});
