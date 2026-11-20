// PostToolUse hook (Edit|Write): formats the edited file the way CI checks it.
// Rust: `cargo fmt` on the crate. Frontend/config: Prettier, which honours .prettierignore
// (so src/bindings/, *.md and the lock file are left alone). Never blocks the edit.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import path from "node:path";

const input = JSON.parse(readFileSync(0, "utf8"));
const filePath = input.tool_input?.file_path;
const projectDir = process.env.CLAUDE_PROJECT_DIR ?? input.cwd;
if (!filePath || !projectDir) process.exit(0);

const abs = path.resolve(projectDir, filePath);
const rel = path.relative(projectDir, abs);
if (rel.startsWith("..") || path.isAbsolute(rel)) process.exit(0); // outside the repo

const run = (cmd, args) => {
  try {
    // No shell: the file path is passed as a plain argument on every OS.
    execFileSync(cmd, args, { cwd: projectDir, stdio: "ignore" });
  } catch {
    // Formatting failures (e.g. a syntax error mid-edit) surface later in lint/CI.
  }
};

const ext = path.extname(abs);
if (ext === ".rs") {
  run("cargo", ["fmt", "--manifest-path", path.join("src-tauri", "Cargo.toml")]);
} else if (
  [".ts", ".tsx", ".js", ".mjs", ".json", ".css", ".html", ".yml", ".yaml"].includes(ext)
) {
  const prettier = path.join(projectDir, "node_modules", "prettier", "bin", "prettier.cjs");
  run(process.execPath, [prettier, "--write", "--ignore-unknown", abs]);
}

process.exit(0);
