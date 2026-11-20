# Caderno Local: functional analysis for the Rust/Tauri rewrite

> **Note:** written on 2026-11-19 in the legacy repo (`tiagoguerreirodev/Simulados-Concursos`), so file paths refer to that repo. Decisions made afterwards are in [`spec.md`](spec.md); where the two differ (for example §7), the spec wins.

> **Purpose.** This document lists every feature in the current Electron app and sorts each one into **Necessary**, **Optional** or **Unnecessary**, measured against the rewrite's goal: **generate questions with AI agents and store and show performance records.**
>
> **Method.** I read every source file in `electron/`, `src/`, `scripts/`, `tests/`, `launcher/`, `.github/` and the config files, and inspected the bundled data in `biblioteca/estado.json.gz`. Portuguese UI labels appear in *italics* so each item can be found in the current app.
>
> **Status.** Analysis only. No code has been changed.

---

## 1. Categories

| Category | Meaning |
|---|---|
| **Necessary** | Without it the app cannot generate questions, let you answer them, or record and show performance. Also covers anything needed to keep the existing question bank. |
| **Optional** | Adds real value to the core loop, but the core works without it. Keep or drop each one. |
| **Unnecessary** | Infrastructure or features that don't serve the core loop, exist only to support other unnecessary features, or are dead code and decoration. |

Every feature has an ID (`N-…`, `O-…`, `U-…`) so we can refer to it when we discuss next steps.

---

## 2. The current app at a glance

| Aspect | Today |
|---|---|
| Stack | Electron 40, React 19, TypeScript, Vite, `better-sqlite3`, OpenAI SDK (Responses API), MCP SDK client, `pdfjs-dist`, KaTeX, zod, jose, lucide-react |
| Size | About 204 KB of dense TypeScript (lines are very long, so line counts mislead): 129 KB in `electron/` and 76 KB in `src/`. Plus 28 KB CSS, 58 KB tests and 57 KB of scripts. |
| Process model | Main process (`electron/main.ts`) exposes **47 IPC methods** through a single `window.caderno.call(method, payload)` bridge. The renderer is one React app with 9 pages. |
| Storage | SQLite (`Dados/caderno.sqlite`, WAL) with **16 tables**. Question bodies are stored as JSON blobs. Images are content-addressed files in `Dados/recursos/`. |
| Platforms | Windows 11 x64 (NSIS installer, portable exe, C# launcher) and macOS 12+ (dmg and zip, ad-hoc signed) |
| Language | All UI text and AI prompts are in Brazilian Portuguese |

### 2.1 Data in the repository today

From `biblioteca/estado.json.gz`, the state the app syncs to GitHub:

| Item | Count |
|---|---:|
| Unique questions | 9,368 (all **multiple choice, 5 options**) |
| Collection items (question occurrences) | 9,757 |
| Collections (*cadernos*) | 352 (196 "ALEPA · Programação", 156 "Perito Criminal"), all of kind `bateria` |
| Questions with an explanation | 5,443 |
| Questions with an image | 52 |
| Subjects (area/matter/topic paths) | 344 |
| Syllabus items (*edital*) | 184 |
| Drafts | 3 (2 legacy collections already imported, 1 AI-generated still in review) |
| **Attempts / responses / review state** | **0 / 0 / 0** |

**Implication:** the repository snapshot holds **no performance history yet**. Migrating the question bank is necessary. Migrating attempts only matters if a local `Dados/caderno.sqlite` on one of your machines holds attempts that were never synced.

### 2.2 Architecture (current)

```
Renderer (React, sandboxed)                Main process (Node)
 ├─ Home / Library / Solve                  ├─ Store (SQLite: content, attempts, metrics, learning)
 ├─ Generate / Preview                      ├─ Generator (OpenAI Responses + tool loop: RemNote MCP, official-site fetch)
 ├─ Metrics / Errors / Reviews   ── IPC ──▶ ├─ EssayGrader (OpenAI, strict JSON schema)
 └─ Settings (ChatGPT, RemNote,             ├─ Connections (ChatGPT OAuth/PKCE, token vault, RemNote MCP client)
     GitHub, backup)                        ├─ GitHubSync (+ study-snapshot 3-way merge, manifest hashing)
                                            ├─ Backups (daily, 7 kept, restore)
                                            └─ CloseCoordinator (flush → stop AI → sync → quit)
```

---

## 3. Core domain rules (keep these in the rewrite)

These rules make up the "business logic" the rewrite has to reproduce in Rust, whatever the UI looks like.

### 3.1 Question (`src/contracts.ts → questionSchema`)
- `type`: `multiple` (exactly **5** options), `boolean` (*Certo/Errado*, exactly **2** options), `essay` (0 options).
- Options must have unique IDs and unique text (case-insensitive). For non-essay questions, `answerId` must match one of the options.
- `subjects`: 1–30 paths, each exactly `[area, matter, topic]`. This 3-level taxonomy drives every performance breakdown.
- Other fields: `explanation`, `refs[]`, `image` (hash-named file), `imageAlt`, `annulled`, `difficulty`, `tags[]`, `objectives[]`.
- Essay-only fields: `modelAnswer` (required), `rubric[]` (unique IDs, weights summing to **100**, at most 12 items), `passingScore` (0–1, default 0.7).

### 3.2 Identity and deduplication (`electron/store.ts`)
- **Canonical options:** on import, each option ID becomes `sha256(option text)[0..24]`, and `answerId` is rewritten the same way.
- **Fingerprint:** `sha256([type, trimmed stem, sorted option texts, image, essay fields])`. Two questions with the same fingerprint are the same question, and the existing record wins.
- On a duplicate, the importer keeps the old explanation if one exists and takes the union of `refs` and `subjects`.
- An answer-key conflict (same fingerprint, different answer) **blocks** the import.
- Collection ID = `hash([packageId, setId])`. Importing the same package ID twice does nothing.

### 3.3 Attempt lifecycle
- At most **one active attempt per collection** (unique partial index). Starting again resumes the active one.
- Start: questions **and** options are shuffled with crypto RNG, and each response stores a **snapshot** of the question.
- Before finishing, `answerId`, `explanation`, `refs`, `objectives` and `modelAnswer` are stripped from what the UI receives.
- Each change bumps `revision`. `finish(revision)` refuses a stale revision.
- After finishing, results use the **current** answer key, explanation and annulment, so later corrections apply retroactively. Stem and options come from the snapshot.

### 3.4 Outcome and scoring (`electron/outcomes.ts`)

| Case | kind | points |
|---|---|---|
| Question annulled | `annulled` | excluded from the denominator |
| No answer | `blank` | 0, counted in the denominator |
| Essay answered, not yet graded | `pending` | excluded until graded |
| Essay graded | `correct` if `score/10 ≥ passingScore`, else `wrong` | `score/10` (0–1, proportional) |
| Choice equals the answer key | `correct` | 1 |
| Any other choice | `wrong` | 0 |

### 3.5 Performance metrics (`Store.metrics`)
- Takes **finished** attempts only, filtered by contest, mode and novelty, then keeps the **latest response per question**.
- **Achievement** (*Aproveitamento*) = points ÷ evaluated questions (blanks count as 0).
- **Accuracy** (*Acerto entre respondidas*) = correct ÷ answered.
- **Groups:** every prefix of every subject path (area; area › matter; area › matter › topic), with each question counted once per group. Sorted weakest first. Groups with fewer than 5 answers are labelled "insufficient sample".
- **Bank coverage** = distinct answered questions ÷ non-annulled questions in collections.

### 3.6 Spaced repetition (`nextReview`)
- Intervals in days: `1, 3, 7, 14, 30, 60`. A full hit (`score ≥ 1`) moves the streak up (maximum 6). Anything else resets the interval to 1 day.
- Events are idempotent per `attemptId:position`, and out-of-order or older events are ignored.

---

## 4. Feature inventory by area

### A. AI question generation (`electron/generation.ts`, *Gerar com IA*)

| ID | Feature | What it does | Category |
|---|---|---|---|
| N-A1 | Generation request form | Contest, board, quantity (1–100), type (multiple, boolean, essay), difficulty, free-text distribution of topics, free-text prompt, model | **Necessary** |
| N-A2 | Generation pipeline | Builds system instructions plus a JSON example of the package format. Sends the request and the existing taxonomy to the model through the OpenAI Responses API (streaming, `store:false`). Strips code fences, parses the JSON, assigns ID `gerado-<uuid>` and validates against the schema. | **Necessary** |
| N-A3 | Strict output checks | Rejects the package if the question count differs from the request or a question type differs | **Necessary** |
| N-A4 | Taxonomy reuse | Sends every existing `[area, matter, topic]` path so the model reuses categories, which keeps metrics consistent | **Necessary** |
| N-A5 | Progress and cancel | Progress messages are pushed to the UI. *Cancelar geração* aborts the request. A floating "generation in progress" pill shows while you browse other pages. | **Necessary** |
| O-A1 | Agent tool loop | Up to 15 turns of function calls before the final answer. Needed only if O-A2 or O-A3 is kept. | Optional |
| O-A2 | RemNote notes via MCP | Read-only MCP client to a **local** RemNote server. Allows only 7 tools (`search_docs`, `read_docs`, `read_docs_raw`, `read_folders_raw`, `find_quotes`, `see_text_around_rem`, `load_skill`). Settings take a URL and token, or *Usar configuração existente* reads `~/.codex/plugins/cache/remnote-local/remnote/1.0.0/.mcp.json`. | Optional |
| O-A3 | Official-source fetch tool (`ler_oficial`) | Agent may fetch HTTPS pages from an allowlist (`gov.br`, `jus.br`, `leg.br`, microsoft, oracle, postgresql, sqlite, w3, ietf, rfc-editor). No redirects, 20 s timeout, ≤2 MB, HTML stripped, 120k-character cap. | Optional |
| O-A4 | Attachments | `.pdf` (text extracted with pdf.js, no OCR, fails under 80 characters), `.md`, `.txt`, ≤25 MB each, sent inline as sources. Paths must come from the file dialog. | Optional |
| O-A5 | Exclude answered questions | Sends **every** stem you've already answered and rejects packages that repeat one. This grows without limit. | Optional |
| O-A6 | Second-pass AI review | A second model call writes a short critique (ambiguity, single answer, references), appended to the package notes | Optional |
| O-A7 | Reasoning effort and fast mode | Per-model reasoning level and "priority/fast" service tier, read from the model catalog. The last choice is remembered and the package notes record what was used. | Optional |
| O-A8 | Revise a draft (*Pedir ajustes ao agente*) | Re-runs generation with the previous package as context | Optional |
| O-A9 | Fill missing explanations (*Solicitar comentários ausentes*) | Sends up to N questions without an explanation and expects `updates[{questionId, explanation}]` back | Optional |
| U-A1 | Reuse the bank (*Permitir reutilização do banco*) | Sends up to 150 existing questions for the agent to "reuse". This duplicates what collections and practice already do and works against "generate". | Unnecessary |
| U-A2 | Jobs log (`jobs` table) | Records each generation run. Marks running jobs as interrupted at startup. Settings show the last 10. | Unnecessary |

### B. AI provider connection (`electron/connections.ts`, `src/ai-options.ts`)

| ID | Feature | What it does | Category |
|---|---|---|---|
| N-B1 | Provider sign-in | **"Continue with ChatGPT"**: OAuth 2 + PKCE with a loopback redirect (`127.0.0.1:<random>/auth/callback`, 5-minute timeout). Uses `client_id=dynamic_agent_client` and scope `chatgpt.tokens.use.direct` (usage is charged to the ChatGPT plan, not an API key). Verifies the ID token with JWKS and nonce, refreshes 90 s before expiry, and revokes on logout. | **Necessary** (some provider credential is required, see decision D1) |
| N-B2 | Model catalog and picker | `GET /v1/models`, filtered by `visibility=list`. Parses `supported_reasoning_levels` and `service_tiers`. | **Necessary** (can be simpler) |
| N-B3 | Secure credential storage | Tokens are encrypted with Electron `safeStorage` (Windows DPAPI or macOS Keychain) in a per-profile file, outside the database and backups | **Necessary** |
| O-B1 | *Testar geração* | Sends a real one-line request to mark the connection as "verified" | Optional |
| U-B1 | Legacy credential migration | Copies the vault from an old filename scheme | Unnecessary |

### C. Content intake: drafts, preview, import (`Store.saveDraft/preview/importDraft`, *Avaliar pacote*)

| ID | Feature | What it does | Category |
|---|---|---|---|
| N-C1 | Package format | `schemaVersion:1`, `id`, `title`, `contest`, `board`, `kind`, `questions[]`, `sets[]`, plus `syllabus`, `merges`, `updates`, `notes` | **Necessary** (core fields only) |
| N-C2 | Drafts | Every generated package becomes a draft with status `review`. Nothing enters the bank without confirmation. | **Necessary** |
| N-C3 | Preview screen | Counts (questions, with explanation, images, already in the bank, updates), blocking issues, model notes, 8 questions per page, toggle to reveal answers and explanations | **Necessary** |
| N-C4 | Validation issues | Duplicate IDs, answer-key conflicts (within the package and against the bank), missing images, sets that point to unknown questions, empty package | **Necessary** |
| N-C5 | Import | Canonicalize, fingerprint, deduplicate and merge. Creates collections from `sets` (or one collection for the whole package). Idempotent by package ID. | **Necessary** |
| N-C6 | Discard draft | Marks the draft `discarded` | **Necessary** |
| N-C7 | Question images | Content-addressed files in `recursos/`, served through the `caderno://recursos/` protocol, falling back to the draft's asset folder during preview. The existing bank has 52 images. | **Necessary** |
| N-C8 | **One-time migration of the existing bank** | Not a feature today. The rewrite must import the 9,368 questions, 352 collections and 52 images, from `biblioteca/estado.json.gz` or a local `caderno.sqlite`. | **Necessary** (new) |
| O-C1 | Package `updates` | Correct an answer key, explanation, annulment or classification of an existing question. Metrics and reviews are recomputed. | Optional (required by O-A9) |
| O-C2 | Import an external JSON package (*Importar pacote*) | File dialog, ≤80 MB, images read from a sibling `recursos/` folder. Useful for packages made by an outside agent. | Optional |
| O-C3 | Question versions (`versions` table) | Each collection item points to an immutable version of the question. This is a third copy of the question body, alongside `questions.data` and `responses.snapshot`. | Optional (the response snapshot alone is enough) |
| U-C1 | Taxonomy `merges` and `aliases` | The package can declare equivalent subject paths. The importer writes aliases and **rewrites every question and syllabus row**. | Unnecessary |
| U-C2 | Syllabus (`syllabus` table) | Maps syllabus item codes to subjects. Used only by the "syllabus coverage" metric (U-E2). | Unnecessary |
| U-C3 | Bundled legacy collections (*Acervos disponíveis*: ALEPA and Perito cards) | Loads `acervos/alepa.json` and `perito.json` (10 MB) as drafts for review | Unnecessary (replaced by N-C8) |
| U-C4 | Legacy HTML parser (`electron/legacy.ts`, `npm run seed`) | Rebuilds those packages from HTML in sibling folders outside the repo | Unnecessary |
| U-C5 | Permanent draft retention | Drafts (with the full package JSON, including the 2 legacy packages) are kept and synced forever | Unnecessary (delete a draft on import or discard) |
| U-C6 | Unused question fields | `tags` (always empty), `objectives` (present on 5,054 questions, never shown), per-question `difficulty` (stored, never shown or filtered) | Unnecessary (keep `difficulty` only as a request parameter) |

### D. Library and solving (*Meus cadernos*, `src/Solve.tsx`)

| ID | Feature | What it does | Category |
|---|---|---|---|
| N-D1 | Collection list | Search by title, filter by contest, 12 per page, status badge (in progress, finished, not started) | **Necessary** |
| N-D2 | Collection detail | Start or resume an attempt, list recent finished attempts with scores | **Necessary** |
| N-D3 | Start an attempt | Shuffles questions and options and stores the snapshot (§3.3) | **Necessary** |
| N-D4 | Resume an active attempt | Shown on Home (*Continue de onde parou*) and in the Library | **Necessary** |
| N-D5 | Answer a question | Select or deselect an option, saved on every click | **Necessary** |
| N-D6 | Hide answers until finished | Enforced in the backend (§3.3) | **Necessary** |
| N-D7 | Finish | A confirmation shows answered and blank counts, then locks the responses (revision guard) | **Necessary** |
| N-D8 | Results view | Score ring, correct/wrong/blank counts, per-option right/wrong highlight, explanation and references per question | **Necessary** |
| N-D9 | Abandon | Deletes the active attempt so it never reaches history or metrics | **Necessary** |
| N-D10 | Question navigator | Progress bar plus a numbered map (answered or flagged), 10 questions per page | **Necessary** |
| O-D1 | Modes *estudo* and *prova* | The **only** difference is that the timer can't be paused in *prova*. Both reveal answers only at the end. Also a metrics filter. | Optional |
| O-D2 | Timer | Optional duration (1–1,440 min) showing time remaining or elapsed. Expiry only warns. Pause and resume in *estudo*, and the state survives restarts. | Optional |
| O-D3 | Eliminate options | Strike out an option, reversibly. Eliminating the selected option clears the selection. | Optional |
| O-D4 | Flag for review | Per-question flag shown in the navigator | Optional |
| O-D5 | New vs. seen badge (*Inédita / Já resolvida*) | Based on the `exposure` table | Optional |

### E. Performance records and analytics (*Visão geral*, *Desempenho*)

| ID | Feature | What it does | Category |
|---|---|---|---|
| N-E1 | Persistent attempts and responses | Every finished attempt with each response, choice and outcome | **Necessary** |
| N-E2 | Outcome and scoring rules | §3.4 | **Necessary** |
| N-E3 | Attempt score summary | Correct, wrong, blank, annulled, pending, points, achievement | **Necessary** |
| N-E4 | Performance dashboard | Overall achievement and accuracy, breakdown by area › matter › topic, weakest topics first, small-sample warning | **Necessary** |
| N-E5 | Filter by contest | | **Necessary** |
| N-E6 | Home summary | Question count, achievement, accuracy, achievement per area | **Necessary** (simplified) |
| N-E7 | Per-collection history | Past attempts and scores in the collection detail | **Necessary** |
| O-E1 | Bank coverage | Answered ÷ available questions | Optional |
| O-E2 | Filters by mode and by new vs. repeated | Depend on O-D1 and O-D5 | Optional |
| O-E3 | CSV export | Semicolon-separated UTF-8 with BOM, protected against formula injection, uses the active filters | Optional |
| O-E4 | Exposure record (`exposure` table) | First time each question was answered. It survives history pruning and powers O-D5 and O-A5. | Optional |
| U-E1 | **Cap of 5 finished attempts per collection** | Deletes the 6th oldest attempt and its responses. It exists to bound the sync file size, and it **destroys performance history**, which works against the rewrite's goal. | Unnecessary (keep all attempts) |
| U-E2 | Syllabus coverage (*Cobertura do edital*) | Share of syllabus items with at least one answered question | Unnecessary |

### F. Learning features built on performance data (*Caderno de erros*, *Revisões adaptativas*)

| ID | Feature | What it does | Category |
|---|---|---|---|
| O-F1 | Error notebook | Questions whose last result scored below 1, optionally with blanks. A full hit removes the question. | Optional |
| O-F2 | Spaced repetition | `review_state` per question with the 1/3/7/14/30/60-day intervals (§3.6). Lists due and upcoming reviews. | Optional |
| O-F3 | Practice sessions | Builds a synthetic collection (*Caderno de erros · prática* or *Revisão adaptativa · prática*, 1–100 questions) from errors or due reviews, then starts an attempt | Optional |
| O-F4 | Review recomputation | `refreshReview` after answer-key changes and when merging histories | Optional (needed only with O-F2 and O-C1) |

### G. Essay questions (*Discursivas*)

| ID | Feature | What it does | Category |
|---|---|---|---|
| O-G1 | Essay type | Schema fields (model answer, weighted rubric, passing score). The generator prompt includes rubric instructions. | Optional |
| O-G2 | Essay writing | Textarea with a queued autosave, unsaved-changes tracking, *Salvar novamente* retry, and close blocked while unsaved | Optional |
| O-G3 | AI grading | Strict JSON-schema output (per-criterion score 0–10 and feedback, summary, strengths, improvements). The app computes the weighted score. Idempotent. Stops at the first provider failure so it doesn't keep spending quota. Runs automatically on finish, with a manual *Corrigir com ChatGPT* retry, a retry when the network comes back, and a UI poll every 2.5 s. | Optional |
| O-G4 | Pending corrections | Essays awaiting grading are listed on Home and left out of score denominators | Optional |

### H. Content rendering (`src/RichText.tsx`)

| ID | Feature | What it does | Category |
|---|---|---|---|
| N-H1 | Safe text rendering | Imported or generated text is never treated as HTML | **Necessary** |
| O-H1 | LaTeX via KaTeX | `$…$`, `$$…$$`, `\(…\)`, `\[…\]`, rendered offline, with a size cap and error fallback. Only **4** of the 9,368 questions use it, but the generator prompt asks for LaTeX. | Optional (cheap to keep) |
| O-H2 | Inline code and code blocks | Backtick spans. **0** questions use them today. Possibly useful for programming topics. | Optional |

### I. GitHub sync and multi-device (`github-sync.ts`, `study-snapshot.ts`, `github-auth.ts`, `close-coordinator.ts`, `GitHubSettings.tsx`)

The largest subsystem: about **40 KB of the roughly 204 KB of app code**, plus 22 KB of tests and most of the lifecycle logic in `main.ts`.

| ID | Feature | What it does | Category |
|---|---|---|---|
| U-I1 | GitHub authentication | Reads a token with `git credential fill` (with macOS git-binary discovery) or accepts a personal access token. Checks push permission on a **hard-coded repository**. | Unnecessary |
| U-I2 | Full-state snapshot | Exports all 14 study tables as JSON, then gzips it to `biblioteca/estado.json.gz` with a manifest of SHA-256 hashes, Git blob SHAs and counts | Unnecessary |
| U-I3 | Three-way merge | Merges base, local and remote row by row, with special rules for some tables. Conflicts are saved locally and in the repo, and resolved with "prefer local" or "prefer GitHub". | Unnecessary |
| U-I4 | Git Data API transport | Writes blobs, a tree and a commit, then fast-forwards the ref with up to 3 retries on races. Endpoints are allowlisted. | Unnecessary |
| U-I5 | Hash checkpoint cache | Skips downloads when nothing changed (cached `total_changes`, `data_version`, image stats) | Unnecessary |
| U-I6 | Bootstrap from the clone | On first run with an empty database, restores the bank from `biblioteca/` | Unnecessary (replaced by N-C8) |
| U-I7 | Sync lock and overlay | Blocks all edits while syncing at startup and on close | Unnecessary |
| U-I8 | Close coordinator | Flushes renderer saves, stops AI work, syncs, and shows retry / stay open / close offline dialogs | Unnecessary (only a simple "flush unsaved essay" step stays if O-G2 is kept) |
| U-I9 | Study data stored in the code repository | `biblioteca/` (16 MB) and `acervos/` (22 MB) are committed next to the source code | Unnecessary |

> If you need multi-device use, there are much simpler options than a GitHub-backed merge engine: keep the SQLite file in a folder you choose (iCloud, OneDrive or Dropbox) with single-writer use, or add a manual *export/import my data* file. This is decision D3.

### J. Data safety and persistence (`backups.ts`, `migration.ts`, `store.ts`)

| ID | Feature | What it does | Category |
|---|---|---|---|
| N-J1 | Local SQLite store | WAL mode, foreign keys, transactions per mutation | **Necessary** |
| N-J2 | Schema version guard | Refuses to open a database created by a newer app version | **Necessary** (cheap) |
| O-J1 | Automatic daily backup | Online SQLite backup plus images plus a hash manifest. Written to an `.incompleto` folder and renamed when complete. Keeps 7. Configurable folder. | Optional |
| O-J2 | Manual backup and restore | Restore checks manifest hashes, `integrity_check` and schema version, and makes a safety copy before replacing | Optional |
| O-J3 | Open the data folder | | Optional |
| U-J1 | v1→v2 migration snapshot | Upgrade path for old databases | Unnecessary (new schema, one-time import) |

### K. Desktop shell and platform (`main.ts`, `platform.ts`, `preload.ts`)

| ID | Feature | What it does | Category |
|---|---|---|---|
| N-K1 | Command bridge | One generic `call(method, payload)` over 47 methods, with an origin check | **Necessary** (becomes typed Tauri commands, far fewer of them) |
| N-K2 | Window hardening | Strict CSP, path-traversal-safe custom protocol, navigation, new windows and permissions all denied | **Necessary** (in Tauri: CSP plus capabilities) |
| N-K3 | Native dialogs | File picker for attachments and packages, save dialog for CSV, confirm dialogs | **Necessary** (subset) |
| O-K1 | Single-instance lock | Focuses the existing window | Optional (Tauri plugin) |
| O-K2 | Localized macOS menu | | Optional |
| U-K1 | Multi-mode path resolution | Portable marker file, `CADERNO_DATA_DIR`, `CADERNO_REPOSITORY_DIR`, `CADERNO_PORTABLE_ROOT`, `.caderno.json` lookup, data folder next to the clone | Unnecessary (one app-data folder) |
| U-K2 | Test-profile environment switches | `CADERNO_TEST_PROFILE`, `CADERNO_DISABLE_GITHUB_SYNC` | Unnecessary |

### L. Distribution and tooling

| ID | Feature | What it does | Category |
|---|---|---|---|
| O-L1 | Installers | electron-builder: NSIS, portable exe, dmg, zip, hardened runtime, entitlements, optional notarization | Optional (the Tauri bundler replaces it, nothing to port) |
| O-L2 | Manual CI workflow | macOS arm64 and Windows x64 build and test, optional release publish | Optional (rewrite for Tauri) |
| U-L1 | Windows C# launcher (`Abrir-Caderno.exe`, `.cmd`) and macOS `.command` | Downloads the latest portable build from GitHub Releases using your git credential, then starts it next to the clone | Unnecessary |
| U-L2 | Release scripts | `publish-release.mjs`, `SHA256SUMS.txt`, `verify-hash-release.mjs` | Unnecessary |
| U-L3 | Playwright end-to-end and verification scripts | `smoke`, `integration`, `connect`, `prepare-delivery`, `verify-*` (12 files) | Unnecessary (tied to Electron) |
| U-L4 | Diagnostics and maintenance scripts | `benchmark-sync`, `upgrade-sync-manifest`, `export-repository`, `check-*` (one hard-codes `C:/Users/tiago/...`), `inspect-model-options`, `seed-post-mvp` | Unnecessary |
| U-L5 | Unit tests (`tests/`, 70 cases, run with `node:test` inside Electron) | | Unnecessary as code, but **their test names are a good behavior spec** for the Rust tests (e.g. "import is explicit, idempotent and normalizes options"). |

### M. UI chrome (`src/main.tsx`, `src/style.css`)

| ID | Feature | What it does | Category |
|---|---|---|---|
| N-M1 | Navigation | Sidebar with pages, breadcrumb, toasts (auto-dismiss after 14 s), loading and empty states, modal dialogs | **Necessary** |
| N-M2 | Settings: AI connection panel | | **Necessary** |
| O-M1 | Settings: RemNote, data folder and backup panels | Kept or dropped together with O-A2, O-J1–O-J3 | Optional |
| U-M1 | Decoration | Hero illustration (orbits, paper mock-up), motivational sidebar card and slogans, hard-coded avatar "T" and *Meu espaço*, footer tagline, "offline" badge, sync status in the top bar | Unnecessary |
| U-M2 | Settings: GitHub panel, jobs list | | Unnecessary |

---

## 5. Summary by category

### Necessary: the minimal core (41 items)
- **Generate:** N-A1 to N-A5 (form, pipeline, strict checks, taxonomy reuse, progress and cancel), N-B1 to N-B3 (provider sign-in, model picker, secure token storage).
- **Intake:** N-C1 to N-C8 (package format, drafts, preview, validation, dedup import, discard, images, **one-time migration of the existing bank**).
- **Solve:** N-D1 to N-D10 (library, start, resume, answer, hidden answers, finish, results, abandon, navigator).
- **Record and show performance:** N-E1 to N-E7 (attempts and responses, scoring rules, summaries, dashboard by area › matter › topic, contest filter, home summary, history).
- **Platform:** N-H1, N-J1, N-J2, N-K1 to N-K3, N-M1, N-M2.

### Optional: decide one by one (40 items)
- **Agentic sources:** O-A1 to O-A4 (tool loop, RemNote MCP, official-site fetch, PDF/MD/TXT attachments)
- **Generation extras:** O-A5 to O-A9, O-B1, O-C1 to O-C3
- **Solving extras:** O-D1 to O-D5 (modes, timer, eliminate, flag, new/seen)
- **Analytics extras:** O-E1 to O-E4
- **Learning:** O-F1 to O-F4 (error notebook, spaced repetition, practice sessions)
- **Essays:** O-G1 to O-G4 (essay type and AI grading pipeline)
- **Rendering:** O-H1, O-H2
- **Safety and shell:** O-J1 to O-J3, O-K1, O-K2, O-L1, O-L2, O-M1

### Unnecessary: drop (30 items)
- **The whole GitHub sync subsystem:** U-I1 to U-I9 (about 20% of app code)
- **Legacy and migration paths:** U-C3, U-C4, U-J1, U-B1, U-K1, U-K2
- **Complexity with little payoff:** U-C1 (taxonomy merges and aliases), U-C2 and U-E2 (syllabus), U-A1 (bank reuse), U-A2 (jobs log), U-C5 (permanent drafts), U-C6 (dead fields)
- **Harmful to the goal:** U-E1 (5-attempt history cap)
- **Tooling:** U-L1 to U-L5
- **Decoration:** U-M1, U-M2

---

## 6. Findings worth knowing before the rewrite

1. **The history cap deletes performance data.** After the 6th finished attempt, the oldest attempt and its responses are removed from the collection (U-E1). The cap exists only to keep the synced snapshot small.
2. **Question bodies are stored three times:** `questions.data`, `versions.data` and every `responses.snapshot` (O-C3). A leaner schema could keep the current question plus one snapshot per response.
3. **N+1 queries and quadratic work in metrics.** `Store.metrics()` re-reads and parses each question's JSON per row. Syllabus coverage loops over syllabus items × answered questions × subjects. It works at 9k questions but scales poorly.
4. **"Exclude answered" sends every answered stem in the prompt** (O-A5). Token usage grows without limit as history grows.
5. **Markdown is never rendered.** The AI review note (O-A6) comes back in Markdown (`**Parecer:**`, bullet lists) and is shown as raw text.
6. ***Modo estudo* gives no immediate feedback.** It differs from *prova* only in timer pausing (O-D1). If you want per-question feedback while studying, that would be a new feature.
7. **Dead data:** `difficulty`, `tags` and `objectives` are stored per question but never shown or used. `settings.taxonomy` is returned to the UI but never displayed.
8. **The provider sign-in depends on a special flow.** "Sign in with ChatGPT" uses `client_id=dynamic_agent_client`, scope `chatgpt.tokens.use.direct` and an agent-host ID, so generation is charged to the ChatGPT plan. The model catalog fields (`slug`, `supported_reasoning_levels`, `service_tiers`) also differ from the public API's. Plain OAuth+PKCE is easy to rebuild in Rust, but before porting we should confirm the flow still works for a new client (decision D1).
9. **The drafts table holds about 10 MB** of the two legacy packages, which are already imported and still synced (U-C5).
10. **Hard-coded personal values:** repository `tiagoguerreirodev/Simulados-Concursos`, username in git credential calls, and the `C:/Users/tiago/...` path in `scripts/check-remnote.ts`.

---

## 7. Preliminary implications for the rewrite (to discuss)

### 7.1 Frontend recommendation: **Solid.js**
- The core screens hold a lot of interactive state that changes often. The solve screen alone has per-question selection, optional eliminations and flags, a once-per-second timer, save status, pagination and a navigator. On top of that come streamed generation progress, filter-driven metric queries, paginated lists and modals. With plain HTML/JS all of that becomes hand-written DOM updates.
- Solid updates only the parts of the page that change, with no virtual DOM and a runtime of about 7 KB. It uses JSX, so the existing React components (`Solve`, `Preview`, `Metrics`, `Learning`) port nearly line for line.
- It works with Vite (`vite-plugin-solid`), Tauri's `invoke` and events, and KaTeX if O-H1 stays.
- Plain HTML/CSS/JS would make sense only if the scope shrinks to the bare Necessary set and you want no build step. Even then, the solve and preview screens would be the most fragile part of the code.

### 7.2 What a Necessary-only backend looks like (Rust)
- **Tables:** `questions`, `subjects` and `question_subjects`, `collections` and `collection_items`, `attempts`, `responses` (with snapshot), `drafts` (transient), `settings`. That's 8 tables instead of 16.
- **Commands:** about 20 instead of 47: home, collections, collection, start, attempt, answer, finish, abandon, metrics, generate, cancel, drafts, preview, import, discard, plus sign-in, status, sign-out and models.
- **Likely crates:** `rusqlite` (bundled), `serde`/`serde_json`, `reqwest` with SSE streaming for the Responses API, `keyring` for tokens, `sha2`, `oauth2` or hand-rolled PKCE, `jsonwebtoken` or `josekit` for ID-token checks, and Tauri plugins `dialog`, `opener` and `single-instance`. Add `pdf-extract` if O-A4 stays and an MCP client (e.g. `rmcp`) if O-A2 stays.

### 7.3 Decisions for our next conversation
| # | Decision |
|---|---|
| D1 | **AI provider and auth:** keep "Sign in with ChatGPT" (plan-billed, special flow), switch to an API key (simpler, billed separately), or support both? |
| D2 | **Which Optional groups to keep:** agent tools (RemNote, official sites, attachments), essays and AI grading, learning (error notebook, spaced repetition), timer and modes, backups. |
| D3 | **Multi-device:** drop it, use a synced-folder database, or offer a manual export/import file? |
| D4 | **Data migration source:** `biblioteca/estado.json.gz` from the repo, or a local `Dados/caderno.sqlite` with possible unsynced attempts? |
| D5 | **Repository layout:** rewrite in place (new branch) or a new repo, and should the 38 MB of study data leave the code repository? |
| D6 | **UI language:** keep Portuguese-only text, or add i18n? |

---

## Appendix: the current 47 IPC methods by category

| Category | Methods |
|---|---|
| **Necessary** | `home`, `collections`, `collection`, `attemptStart`, `attempt`, `answer`, `finish`, `abandon`, `history`, `metrics`, `generate`, `cancelGeneration`, `drafts`, `preview`, `import`, `discard`, `settings`, `authStart`, `authStatus`, `authLogout`, `models` |
| **Optional** | `eliminate`, `review` (flag), `pauseTimer`, `essayAnswer`, `gradeEssays`, `learning`, `practice`, `exportCsv`, `openPackage`, `chooseFiles`, `testAI`, `remnoteConnect`, `remnoteTest`, `backup`, `restore`, `chooseBackup`, `openData` |
| **Unnecessary** | `githubStatus`, `githubUseGit`, `githubToken`, `githubDisconnect`, `githubSync`, `githubResolve`, `githubOpen`, `githubTokenHelp`, `legacyPreview` |
