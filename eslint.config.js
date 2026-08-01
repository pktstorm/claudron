import js from "@eslint/js";
import globals from "globals";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";

export default tseslint.config(
  // `.remember` is a scratch directory outside any tsconfig; the type-aware
  // parser errors on files it cannot resolve to a project.
  { ignores: ["dist", "src-tauri/target", ".yarn", ".remember"] },
  {
    // `recommended` plus only the type-aware rules worth their noise. The full
    // `recommendedTypeChecked` set reports ~28 issues here, nearly all
    // `no-unsafe-*` on deliberately-partial test fixtures -- a signal-to-noise
    // ratio that gets a linter ignored rather than obeyed.
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    files: ["**/*.{ts,tsx}"],
    languageOptions: {
      ecmaVersion: 2022,
      globals: globals.browser,
      // Type information is required for prefer-nullish-coalescing to tell a
      // real fallback from a boolean OR. Without it the rule either misses the
      // bugs or flags every `a === x || a === y`, which trains people to
      // disable it.
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
    plugins: {
      "react-hooks": reactHooks,
      "react-refresh": reactRefresh,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "react-refresh/only-export-components": ["warn", { allowConstantExport: true }],

      // `??` over `||` is a project rule, not style: 0 and "" are legitimate
      // values that `||` discards, and `ahead: 0` ("in sync") versus
      // `ahead: null` ("no upstream") is a distinction this app renders
      // differently. Type-aware, so a genuine boolean OR is left alone.
      "@typescript-eslint/prefer-nullish-coalescing": "error",

      // Tests deliberately construct partial fixtures and assert on shapes.
      "@typescript-eslint/no-explicit-any": "warn",
    },
  },
  {
    // Vitest globals are enabled in vite.config.ts, not imported.
    files: ["**/*.test.{ts,tsx}"],
    languageOptions: { globals: { ...globals.browser, ...globals.node } },
  },
);
