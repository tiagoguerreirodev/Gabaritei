# Gabaritei

A personal desktop app that **generates exam questions with AI agents**, checks them blind with a second model, and **tracks your performance** by area › matter › topic. Built for studying for Brazilian public exams (*concursos*). The interface is in Portuguese.

> **Status:** early development. The app shell runs, but no features work yet. Work is tracked in [Issues](../../issues) and grouped in milestones `M0`–`M7`.

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
