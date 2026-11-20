// @ts-check
import js from "@eslint/js";
import prettier from "eslint-config-prettier";
import { defineConfig } from "eslint/config";
import solid from "eslint-plugin-solid/configs/typescript";
import globals from "globals";
import tseslint from "typescript-eslint";

export default defineConfig(
  {
    ignores: ["dist/", "coverage/", "src-tauri/", "src/bindings/"],
  },
  js.configs.recommended,
  // App code: type-aware TypeScript rules plus Solid's reactivity rules.
  {
    files: ["src/**/*.{ts,tsx}"],
    extends: [tseslint.configs.recommendedTypeChecked],
    languageOptions: {
      globals: globals.browser,
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
  },
  {
    files: ["src/**/*.{ts,tsx}"],
    ...solid,
  },
  // Config files run in Node and are not part of the app's tsconfig.
  {
    files: ["*.config.{js,ts}"],
    extends: [tseslint.configs.recommended],
    languageOptions: { globals: globals.node },
  },
  // Claude Code hook scripts run in Node.
  {
    files: [".claude/hooks/*.mjs"],
    languageOptions: { globals: globals.node },
  },
  // Must stay last: turns off rules that conflict with Prettier.
  prettier,
);
