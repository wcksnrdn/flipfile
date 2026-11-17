import js from "@eslint/js";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";
import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["dist", "src-tauri", "flipfile-promo", "node_modules"] },
  {
    files: ["src/**/*.{ts,tsx}"],
    extends: [js.configs.recommended, ...tseslint.configs.recommended, reactHooks.configs.flat.recommended],
    languageOptions: { globals: globals.browser },
    rules: {
      // Unused values are already a type error (noUnusedLocals / noUnusedParameters)
      "@typescript-eslint/no-unused-vars": "off",
      // A missing effect dependency is how stale closures happen: fail the build, don't warn
      "react-hooks/exhaustive-deps": "error",
    },
  },
  {
    files: ["scripts/**/*.{js,mjs}", "*.config.js"],
    extends: [js.configs.recommended],
    languageOptions: { globals: globals.node },
  },
  {
    files: ["*.config.ts"],
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    languageOptions: { globals: globals.node },
  }
);
