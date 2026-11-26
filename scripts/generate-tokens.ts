/// <reference types="node" />
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { renderTokensCss, type DesignTokens } from "./tokens.ts";

const SOURCE = "docs/design-system/tokens.json";
const TARGET = "src/styles/tokens.css";

const tokens = JSON.parse(readFileSync(SOURCE, "utf8")) as DesignTokens;
writeFileSync(TARGET, renderTokensCss(tokens));
execFileSync("npx", ["prettier", "--write", TARGET], {
  stdio: "inherit",
  shell: process.platform === "win32",
});
