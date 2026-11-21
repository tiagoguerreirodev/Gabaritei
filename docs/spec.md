# Gabaritei: product spec

> **Status:** decisions from the 2026-11-19 interview with the owner. This is the source of truth for Gabaritei. Changing a decision needs the owner's approval; record the change here in the same PR.
> **Companion:** [`legacy-analysis.md`](legacy-analysis.md), the inventory of the legacy Electron app (*Caderno Local*, repo `tiagoguerreirodev/Simulados-Concursos`). Feature IDs like `N-D3` refer to it.

## 1. What the app is

A personal desktop app for **one person (you) on two computers**. It:

1. **Generates** exam questions with AI agents, using your own Claude Code and Codex subscriptions.
2. **Checks** them blind with a second, stronger model before they enter your bank, without ever showing you the answers in advance.
3. Lets you **solve** questions by collection, by topic, or from your errors and due reviews.
4. **Records performance**: every answer, kept forever, with a breakdown by area › matter › topic and a trend over time.

All UI text is in Portuguese. There are no accounts and no server.

---

## 2. Decision record

| # | Topic | Decision |
|---|---|---|
| 1 | AI access | Run the **locally installed, already-logged-in `claude` and `codex` CLIs** as subprocesses with schema-checked JSON output. The app stores **no AI credentials**. |
| 2 | AI roles | Four roles: **Generate, Blind check, Re-check on demand, Explain on demand**. Each has its own **agent (Claude or Codex), model and reasoning effort**. Defaults live in Settings; Generate and Blind check can be overridden in the generation form. Intended use: a cheaper model generates, a smarter model checks. |
| 3 | Devices | Two computers, switching rarely. Data moves with a **manual export/import file**. No live sync. |
| 4 | Users | Only you. No onboarding, no public distribution concerns. |
| 5 | History migration | **Bank only.** Import questions, collections, topics and images once from this repo's `biblioteca/`. There is no attempt history to carry over. |
| 6 | Question formats | **5-option multiple choice** and **Certo/Errado**. No essays. |
| 7 | Taxonomy | One **topic tree** (area › matter › topic). **Contest** and **board** become optional tags, not the top-level grouping. Tags live on **collections**; a question's contests are those of the collections that contain it (decided 2026-11-19, because 389 legacy questions belong to both contests). |
| 8 | Generation scope | **One topic per generation**, picked from the tree or typed as a new path. Every generated question gets exactly that topic. |
| 9 | Generation form | Topic and count are required. Optional: **board style**, **format** (blank means the AI may mix both formats), **free-text instructions**. Sources: **RemNote**, **attached files**, **web**. The generator also gets **that topic's existing stems** so it avoids near-duplicates. |
| 10 | No spoilers | **No screen shows a question's answer before you've answered it.** That includes the package preview. |
| 11 | Package review | Preview shows the **summary and blind-check verdicts only**: count, topic, validation problems, and agrees/disputed per question. No stems, options or answers. You approve or discard. |
| 12 | Blind check | The checker sees **only stems and options**, never the key or explanation, and answers each question. **Disputes:** the generator gets the objection **once** to fix or defend the question. Anything still disputed is **flagged** in the preview. |
| 13 | Feedback timing | **Chosen per attempt:** *Estudo* reveals the answer and explanation right after each answer. *Simulado* reveals everything at the end. |
| 14 | Bad questions | While solving, after the answer is revealed: **Annul**, or **Ask AI to re-check**. Re-check shows the verdict, corrected key and sources; **you confirm, then it's applied** and past answers are re-scored. There's no manual text editing. |
| 15 | Missing explanations | **On demand only.** An *Explicar* button on questions that have no explanation. |
| 16 | Sessions | From a **collection**, by **topic** (N questions, unseen first), or from **errors and due reviews**. |
| 17 | Kept study tools | **Eliminate options** and **flag questions**. **Error notebook** and **spaced repetition** (1/3/7/14/30/60 days). |
| 18 | Dropped study tools | Timer, exam mode with a locked timer, essays and AI grading. |
| 19 | Counting | Topic strength uses the **latest answer per question**: a blank in a newer attempt never hides an earlier answer, and a question counts as blank only if it was never answered. A **weekly accuracy trend** uses all answers. **Every attempt is kept forever**, with no 5-attempt cap. |
| 20 | Safety | **Daily automatic backup, 7 kept**, with a restore button. Plus the export/import file. |
| 21 | Platforms | **Windows 11 x64** and **macOS Apple Silicon**. |
| 22 | Repo | **This repository (Gabaritei).** Study data stays **out of git**: it lives in the app data folder, backups and exports. |
| 23 | Extras | **Keyboard shortcuts**, **dark mode** following the system, **LaTeX** via KaTeX, **CSV export**. |
| 24 | Images | Display the 52 migrated images. **Generated questions are text-only.** |
| 25 | Stack | **Tauri 2 + Rust** backend, **Solid.js + Vite** frontend (see the analysis, §7.1). |

---

## 3. Feature set

### 3.1 Bank and taxonomy
- **Topic tree:** area › matter › topic. Browse it with counts of questions, answered questions and accuracy.
- **Question fields:** `type` (`multiple` or `boolean`), stem, options (5 for multiple choice, Certo/Errado for boolean), answer key, explanation, references, topics (one or more paths), image (migrated only) with alt text, `annulled`, provenance (agent, model, date, blind-check verdict). Questions carry no contest or board of their own; see Collections below.
- **Collections** are fixed question lists with optional contest and board tags: the 352 migrated *cadernos*, plus one collection per approved package.
- **Deduplication** works as today (`N-C5`): canonical option IDs plus a content fingerprint. An answer-key conflict blocks import.
- **Dropped:** `difficulty`, generic `tags`, `objectives`, the versions table, syllabus, taxonomy merges and aliases (`U-C1`, `U-C2`, `U-C6`, `O-C3`).

### 3.2 Generation (*Gerar*)
**Form**
- Topic (tree picker or new path)
- Count
- Board style (optional)
- Format (optional; blank means the AI may mix)
- Free-text instructions (optional)
- Sources: RemNote on/off, attached files (PDF, MD, TXT), web on/off
- Generator agent, model and effort, prefilled from Settings
- Checker agent, model and effort, prefilled from Settings

**Pipeline**
1. **Generate.** Run the generator CLI with the package JSON schema. The input includes the topic path, count, board, format, instructions, the topic's existing stems and the source settings.
2. **Validate.** Check the schema, option rules, duplicates and answer-key conflicts.
3. **Blind check.** Run the checker CLI with **stems and options only**. It returns its chosen option, confidence and short justification per question.
4. **Resolve disputes once.** Send each disagreement (the checker's choice and reasoning) back to the generator, which revises or defends the question. The checker re-checks the revised questions. Whatever still disagrees is marked **disputed**.
5. **Save the draft** and show the preview.

**Run behavior**
- Progress comes from the CLIs' streaming JSON events.
- *Cancelar* kills the subprocess.
- Only one generation runs at a time.

**Preview (*Revisar pacote*)**
- Shows the topic, count, format mix, validation problems and agreed/disputed counts. Each question appears as "Q3 · disputed" with no content.
- Actions: **Import**, **Discard**.

### 3.3 Solving (*Resolver*)
- **Start a session from** a collection; a topic with N questions (unseen first, then least recently answered); or errors and due reviews (`O-F1`–`O-F3`).
- **Feedback mode per attempt:**
  - *Estudo*: choosing an option locks it and reveals the key and explanation for that question.
  - *Simulado*: everything is revealed on finish.
- **Carried over from today:** shuffled questions and options, eliminating options, flags, the navigator, autosave, finish with a confirmation, abandon, one active attempt per collection (`N-D1`–`N-D10`, `O-D3`, `O-D4`).
- **Keyboard:** `A`–`E` (or `C`/`E`) to answer, `←`/`→` to move, `X` plus a letter to eliminate, `F` to flag.
- **After a question is revealed:**
  - **Anular** removes it from all scores.
  - **Pedir revisão à IA** runs the Re-check role. It shows the verdict, the corrected key if any, and sources. Confirming applies the key change, logs it and re-scores past answers.
  - **Explicar** (only when the explanation is missing) runs the Explain role and saves the result to the question.

### 3.4 Performance (*Desempenho*)
- **Scoring:** correct, wrong, blank or annulled (§3.4 of the analysis, without essays or pending grades).
- **Topic strength** uses the latest *answered* response per question, ranked by when it was answered, the same ordering as the error notebook (a later blank never hides an earlier answer; a never-answered question counts as blank). Coverage counts questions answered at least once. It reports achievement, accuracy and coverage by area › matter › topic. Weakest topics come first, with a small-sample warning.
- **Trend:** weekly accuracy from all answers.
- **Filters:** contest tag, board tag (both from the collections a question belongs to), feedback mode.
- **Other views:** history per collection and per session, a home dashboard, and CSV export.
- **All attempts are kept** (drops `U-E1`).

### 3.5 Learning (*Erros e revisões*)
- **Error notebook:** questions whose latest *answered* response is wrong (a later blank never hides an earlier wrong answer, the same blank rule as §3.4); the list links to the attempt holding that wrong answer. Blanks are optional: when included, a question whose latest result is blank is listed too.
- **Spaced repetition:** a full hit moves the interval along 1 → 3 → 7 → 14 → 30 → 60 days (the first hit is 1 day, as in the legacy app). A miss resets it to 1. A blank answer counts as a miss. Only finished attempts count, and annulled questions have no review.
- **Derived state:** a review is recomputed from the question's answers against its current key, so key corrections and annulments apply to it, and it is rebuilt at startup.
- **Practice sessions** are built from errors and due reviews: the reviews session takes due reviews first, then recent errors (1–100 questions). Questions that sit in an unfinished attempt are left out of a new errors or reviews session, so a new errors or reviews session never repeats a question already in progress. The "due now" count still includes them, with a note saying how many are in an open attempt.

### 3.6 Data
- **Storage:** SQLite in the OS app-data folder.
- **Daily backup:** a folder with an online copy of the database, the images and a manifest of sha256 hashes, written to a temporary folder and renamed. Checked at start and every 5 minutes while the app is open (against the wall clock, so time asleep counts); made when the newest is over 24 hours old. Keeps 7, in a configurable folder (default `<app data>/backups`). Backups made by hand ("Fazer backup agora") are kept apart: the newest 7 of them, and they never count as the daily backup or push one out. Backup folder names end with a **computer tag** made at run time from the host name (`backup-20261119-210000-123-desktop-abc`; lowercase letters, digits and hyphens; none when the host name gives nothing usable), so the two computers can share one synced folder: each one rotates (7, 7 and 5) and cleans up only the backups carrying its own tag, never the other's or, on a computer that has a tag, the untagged ones made before tags existed (decided 2026-11-20). Only the part of the host name before the first dot is used, so a network domain doesn't change the tag; a computer whose host name gives nothing usable makes untagged backups and rotates the untagged ones as before. The list shows everything in the folder and marks the other computer's backups, which can be restored; whether a daily backup is due looks only at this computer's. After the folder changes it says how many backups, safety ones included, stay behind in the old folder; nothing is moved or deleted unless the user asks to **copy** or **move** them to the new folder. The offer is remembered across restarts (a local setting, kept by restore and import) until the backups are moved, the user dismisses it, or the old folder holds none of this computer's backups. Each backup is copied to a temporary folder, checked with the same verification as a restore, and renamed; an original is deleted (move only) once its copy is verified, and any failure leaves the original in place. The backup lock is taken per backup, so a scheduled backup can run between two of them (the Dados page blocks other actions meanwhile, and AI jobs are refused with a message that names the copy); if the folder changes meanwhile, the rest stay in the old folder. Moving asks for confirmation first. Another computer's backups in the old folder, and untagged ones on a computer that has a tag (they can't be told apart, as with rotation), are never copied or moved, and a new folder inside the old one is refused. A backup folder inside any backup or temporary build folder is refused too, because rotation could later delete it.
- **Restore:** validates the hashes, `integrity_check` and the schema version first (a bad backup changes nothing), then makes a safety backup (the newest 5 are kept), then replaces the database and adds the images. This computer's own settings (backup folder, CLI paths, RemNote connection, role settings) are kept, and reviews are rebuilt. The backup's database is copied to a private folder, migrated and compared with this build's schema first (an extra or missing table, trigger or view is rejected). Restore is refused while an AI job runs.
- **Export:** one file, `Gabaritei-<date>.gabaritei`: a zip with a consistent DB copy, the images and the manifest of hashes.
- **Import:** unpacks the file into a temporary folder (only the names a snapshot has, with per-file and total size limits), validates and stages it like a backup, makes a safety backup, then replaces local data. An invalid file changes nothing. There's no merging, so use one computer at a time; the UI warns about it. Like a restore, it keeps this computer's backup folder, CLI paths, RemNote connection and role settings, and it is refused while an AI job runs.
- **First-run migration:** imports the bank from `biblioteca/estado.json.gz` and `biblioteca/recursos/` of the old repo: questions, topics, collections with their contest tags, and images. The legacy board value "Acervo autoral" isn't an exam board, so migrated collections get no board tag. Attempts, drafts and settings are skipped.

### 3.7 Settings (*Configurações*)
- **CLI paths:** detected automatically, with manual override. Apps opened from the macOS Finder see a short PATH, so detection matters there. Includes a "Test" button per CLI that shows its version and login state.
- **Role defaults:** agent, model and effort for each of the 4 roles.
- **RemNote MCP:** URL and token, or reuse the existing Codex config.
- **Data:** backup folder and restore, export/import, open data folder.

---

## 4. AI integration design

### 4.1 Roles
| Role | Input | Output (JSON schema) | Sees the answer key? |
|---|---|---|---|
| Generate | Form, topic stems, sources | Package (questions with key and explanation) | Writes it |
| Blind check | Stems and options only | `{questionId, choice, confidence, reason}[]` | **No** |
| Re-check | One question with key, explanation and refs | `{verdict: correct\|wrong\|ambiguous, correctedKey?, reasoning, sources[]}` | Yes, after you've answered |
| Explain | One question with key | `{explanation, refs[]}` | Yes, after you've answered |

### 4.2 Running the CLIs

The patterns below were **verified against the installed CLIs** (Claude Code 2.1.329, codex-cli 0.171.0); the exact flags and behavior are recorded in [`docs/ai-cli.md`](ai-cli.md), which wins over the sketches here.

- **Claude Code:** `claude -p "<prompt>" --output-format json --json-schema '<schema>' --model <m> --permission-mode dontAsk --allowedTools "<read-only list>" --mcp-config <remnote.json> --append-system-prompt "<role instructions>"`
  - The result is in the `structured_output` field. Check that the field is present before using it.
  - Use `--output-format stream-json --verbose` for progress.
  - Don't use `--bare`, because bare mode skips the subscription login.
  - Without `--bare`, Claude Code loads `~/.claude` hooks, plugins and `CLAUDE.md`, and the working directory's `.claude/` and `.mcp.json`. Run it in an empty temporary folder, and check for a flag that limits which settings sources load.
  - The exact flag for effort still needs confirming.
- **Codex:** `codex exec "<prompt>" --output-schema <schema.json> -o <out.json> --json -m <model> -c model_reasoning_effort=<e> --sandbox read-only --skip-git-repo-check --ephemeral`
  - Codex runs read-only by default.
  - RemNote comes from your Codex config, so `--ignore-user-config` can't be used.
  - Still to confirm: whether web search needs a config switch, and the exact `-m` and effort keys.
- **Working folder:** each run gets a fresh temporary folder. Attachments are copied into it and the agent reads them itself, so the app has no PDF parser.
- **Tools:** read-only only, meaning file read, web search and fetch, and the RemNote read tools. No file edits and no shell, except Codex's shell inside its read-only sandbox, which is kept only when a run has attachments (owner-approved, 2026-11-20). Sources and tool results are treated as data, never as instructions.
- **Cancel and timeout:** SIGINT to the process group, then SIGKILL after a short grace period (on Windows the process tree is ended with `taskkill /T /F`).
- **Policy:** the app never handles Claude or OpenAI credentials. It runs the CLI you logged into yourself. Anthropic doesn't allow third-party products to offer claude.ai login, and this app doesn't.

---

## 5. Architecture sketch

**Rust crates and modules**
- `db`: rusqlite, migrations
- `bank`: questions, topics, collections, dedup, import
- `attempts`: sessions, feedback modes, scoring
- `metrics`: strength, trend, CSV
- `learning`: errors, spaced repetition
- `ai`: runner, `claude`, `codex`, prompts, schemas, roles
- `backup`: daily, restore, export/import
- `migrate`: legacy bank import

**Tauri commands, about 25:**
- `home`
- `topics_tree`
- `collections`, `collection`
- `session_start`, `attempt`, `answer`, `eliminate`, `flag`, `finish`, `abandon`
- `annul`, `recheck`, `apply_recheck`, `explain`
- `metrics`, `trend`, `export_csv`
- `learning`
- `generate`, `cancel`, `draft`, `import_draft`, `discard_draft`
- `settings_get` / `settings_set`, `cli_test`
- `backup_status`, `backup_now`, `backup_choose_folder`, `backup_use_default_folder`, `backup_restore`, `export_data`, `import_data`

**Events:** generation progress, re-check and explain progress.

**Frontend pages (Solid):** *Início*, *Banco*, *Gerar*, *Revisar pacote*, *Resolver*, *Desempenho*, *Erros e revisões*, *Configurações*.

**Tables:**
- `topics`, `questions`, `question_topics`
- `collections`, `collection_items`
- `attempts` (source, feedback mode)
- `responses` (snapshot, choice, eliminated, flagged, revealed_at)
- `review_state`
- `drafts`
- `key_changes` (re-check audit)
- `settings`

That's 11 tables, down from 16.

---

## 6. Explicitly out of scope
- **Sync and credentials:** GitHub sync (`U-I*`), ChatGPT OAuth and the token vault (`N-B1`'s current form), the model catalog.
- **AI plumbing:** the in-app tool loop, the PDF extractor, the website-fetch allowlist tool, bank reuse (`U-A1`), the jobs log (`U-A2`).
- **Study features:** essays and AI grading (`O-G*`), the timer and exam mode (`O-D1`, `O-D2`), the syllabus, taxonomy merges.
- **Legacy and distribution:** legacy collection cards, the C# launcher, release scripts, Electron test scripts.
- **UI:** decoration, i18n.

## 7. Proposed defaults (change any you disagree with)
| Item | Proposed default |
|---|---|
| Count per generation | 1–30. Each question goes through the blind check, so large batches are slow. |
| Import with disputed questions | Two buttons: **Import all** (disputed questions get a badge after you answer) and **Import agreed only**. |
| Topic session order | Unseen questions first, then least recently answered, with annulled questions excluded |
| Role defaults | Owner decision (2026-11-20): **every role defaults to Codex `gpt-6.2-sol` at extra-high effort** (Generate, Blind check, Re-check and Explain). Set on first run in Settings and editable per role. |
| `Estudo` lock rule | Once revealed, a question's choice can't change |
| Export file name | `Gabaritei-<date>.gabaritei` (a zip) |
