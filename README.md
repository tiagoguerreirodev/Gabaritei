# Gabaritei

A personal desktop app that **generates exam questions with AI agents**, checks them blind with a second model, and **tracks your performance** by area › matter › topic. Built for studying for Brazilian public exams (*concursos*). The interface is in Portuguese.

> **Status:** core features are in place, some M4–M6 issues are still open, and M7 covers packaging and Windows verification. Work is tracked in [Issues](../../issues) and grouped in milestones `M0`–`M7`.

## How it works
- **Generation:** you pick a topic and a count, plus optional board style, format and instructions. Your locally installed **Claude Code** or **Codex** CLI writes the questions using your own subscription. It can read your RemNote notes, attached files and the web.
- **Blind check:** a stronger model answers every new question without seeing the answer key. Disagreements get one automatic fix, and anything still disputed is flagged. The preview never shows answers.
- **Practice:** solve by collection, by topic, or from your errors and due spaced-repetition reviews. Choose per attempt whether answers are revealed right away (*Estudo*) or at the end (*Simulado*).
- **Performance:** strength per topic uses your latest answer to each question, plus a weekly accuracy trend. Every attempt is kept.
- **Data:** a local SQLite database, daily backups, and an export/import file for moving between computers. Study data never goes into git.

## Docs
- [`docs/spec.md`](docs/spec.md): product spec and decision record
- [`docs/legacy-analysis.md`](docs/legacy-analysis.md): what the previous Electron app (*Caderno Local*) did and what was kept
- [`AGENTS.md`](AGENTS.md): rules and workflow for AI coding agents (and humans)

## Stack
Tauri 2 (Rust) · Solid.js + TypeScript + Vite · SQLite (rusqlite) · KaTeX. Targets Windows 11 x64 and macOS on Apple Silicon.

## Install
Installers are built by hand: in **Actions**, run the *Release build* workflow and download the artifacts. The version comes from `package.json` (keep it equal to `src-tauri/Cargo.toml`; a test checks this).
- **Windows 11 x64:** run the `.exe` (NSIS). It installs for the current user only, with no admin prompt. Windows SmartScreen may warn about an unknown publisher: choose *More info* › *Run anyway*.
- **macOS (Apple Silicon):** open the `.dmg` and drag Gabaritei to *Applications*. The app is only ad-hoc signed (no notarization), so macOS blocks the first launch. Run `xattr -dr com.apple.quarantine /Applications/Gabaritei.app` once (or, on macOS 15 and later, use System Settings › Privacy & Security › *Open Anyway*).
- There is no auto-updater: install a newer build over the old one. Data lives in the OS app-data folder, so it survives reinstalling.
- For the AI features, install and log in to `claude` and/or `codex` first.

## Development
Prerequisites:
- Rust (stable) and the [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS
- Node.js 26 with npm (see `.nvmrc`)
- To use AI features: the `claude` and/or `codex` CLIs installed and logged in

```sh
npm install          # once
npm run tauri dev    # run the app

npm test && npm run lint && npm run typecheck
cargo test --manifest-path src-tauri/Cargo.toml
```

[`AGENTS.md`](AGENTS.md#commands) has the full command list.
