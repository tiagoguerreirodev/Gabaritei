/**
 * The only place the frontend talks to the Rust backend.
 *
 * Types come from `src/bindings/`, which `cargo test` generates from the Rust structs
 * (ts-rs). Add one function per Tauri command here; never call `invoke` elsewhere.
 */
import { invoke, type InvokeArgs } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Agent } from "./bindings/Agent";
import type { AiSettings } from "./bindings/AiSettings";
import type { AiSettingsInput } from "./bindings/AiSettingsInput";
import type { AppInfo } from "./bindings/AppInfo";
import type { AttemptView } from "./bindings/AttemptView";
import type { BackupInfo } from "./bindings/BackupInfo";
import type { BackupStatus } from "./bindings/BackupStatus";
import type { RelocateOutcome } from "./bindings/RelocateOutcome";
import type { CliStatus } from "./bindings/CliStatus";
import type { CollectionDetail } from "./bindings/CollectionDetail";
import type { CollectionPage } from "./bindings/CollectionPage";
import type { CollectionQuery } from "./bindings/CollectionQuery";
import type { ErrorPayload } from "./bindings/ErrorPayload";
import type { GenerateRequest } from "./bindings/GenerateRequest";
import type { GenerationStatus } from "./bindings/GenerationStatus";
import type { DraftListItem } from "./bindings/DraftListItem";
import type { DraftSummary } from "./bindings/DraftSummary";
import type { LearningOverview } from "./bindings/LearningOverview";
import type { HomeSummary } from "./bindings/HomeSummary";
import type { ImportMode } from "./bindings/ImportMode";
import type { ImportResult } from "./bindings/ImportResult";
import type { LegacyImportOutcome } from "./bindings/LegacyImportOutcome";
import type { LegacyImportStatus } from "./bindings/LegacyImportStatus";
import type { OpenableFolder } from "./bindings/OpenableFolder";
import type { Performance } from "./bindings/Performance";
import type { PerformanceQuery } from "./bindings/PerformanceQuery";
import type { RemNoteTest } from "./bindings/RemNoteTest";
import type { RecheckView } from "./bindings/RecheckView";
import type { SessionStart } from "./bindings/SessionStart";
import type { TopicTree } from "./bindings/TopicTree";

const FALLBACK_MESSAGE = "Ocorreu um erro inesperado. Tente novamente.";

/** A failed command. `message` is pt-BR and safe to show to the user. */
export class ApiError extends Error {
  override name = "ApiError";
}

function isErrorPayload(value: unknown): value is ErrorPayload {
  return (
    typeof value === "object" &&
    value !== null &&
    "message" in value &&
    typeof value.message === "string"
  );
}

async function call<T>(command: string, args?: InvokeArgs): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw new ApiError(isErrorPayload(error) ? error.message : FALLBACK_MESSAGE);
  }
}

export const api = {
  /** Name and version of the running app. */
  appInfo: () => call<AppInfo>("app_info"),
  /** Whether the legacy Caderno Local bank was imported, and whether the bank is empty. */
  legacyImportStatus: () => call<LegacyImportStatus>("legacy_import_status"),
  /** Opens a folder picker and imports the legacy bank. `null` if the user cancels. */
  importLegacyBank: () => call<LegacyImportOutcome | null>("import_legacy_bank"),
  /** Area › matter › topic with question counts. */
  topicsTree: () => call<TopicTree>("topics_tree"),
  /** One page of collections matching the search and filters. */
  collections: (query: CollectionQuery) => call<CollectionPage>("collections", { query }),
  /** One collection with its attempts. */
  collection: (id: string) => call<CollectionDetail>("collection", { id }),
  /** Home page: summary numbers, attempts to resume, due reviews and pending drafts. */
  home: () => call<HomeSummary>("home"),
  /** Erros e revisões: counts and the list of reviews and errors (no answer keys). */
  learningOverview: (includeBlanks: boolean) =>
    call<LearningOverview>("learning_overview", { includeBlanks }),
  /** Topic strength, weekly trend and filter options. */
  performance: (query: PerformanceQuery) => call<Performance>("performance", { query }),
  /** Asks where to save the performance table as CSV and writes it. `false` if cancelled. */
  exportPerformanceCsv: (query: PerformanceQuery) =>
    call<boolean>("export_performance_csv", { query }),
  /** Starts a session; fails if the collection already has an unfinished attempt. */
  sessionStart: (request: SessionStart) => call<AttemptView>("session_start", { request }),
  /** An attempt; keys and explanations appear only for revealed questions. */
  attempt: (id: string) => call<AttemptView>("attempt", { id }),
  /** Chooses an option, or clears the choice with `null`. */
  answer: (attemptId: string, position: number, optionId: string | null) =>
    call<AttemptView>("answer", { attemptId, position, optionId }),
  /** Toggles an option's elimination. */
  eliminate: (attemptId: string, position: number, optionId: string) =>
    call<AttemptView>("eliminate", { attemptId, position, optionId }),
  /** Sets or clears a question's flag. */
  flag: (attemptId: string, position: number, flagged: boolean) =>
    call<AttemptView>("flag", { attemptId, position, flagged }),
  /** Finishes the attempt; `revision` must be the latest one seen. */
  finish: (attemptId: string, revision: number) =>
    call<AttemptView>("finish", { attemptId, revision }),
  /** Annuls a revealed question: it leaves every score until restored. */
  annul: (attemptId: string, position: number) =>
    call<AttemptView>("annul", { attemptId, position }),
  /** Restores an annulled question to every score. */
  unannul: (attemptId: string, position: number) =>
    call<AttemptView>("unannul", { attemptId, position }),
  /** Deletes an unfinished attempt. */
  abandon: (attemptId: string) => call<null>("abandon", { attemptId }),
  /** Backup folder, the backups that exist (newest first) and the last backup failure. */
  backupStatus: () => call<BackupStatus>("backup_status"),
  /** The app data folder (database and images), to show in Configurações → Dados. */
  dataFolder: () => call<string>("data_folder"),
  /** Opens the app data folder or the backup folder in the system file manager. */
  openFolder: (which: OpenableFolder) => call<null>("open_folder", { which }),
  /** Makes a backup now. */
  backupNow: () => call<BackupInfo>("backup_now"),
  /** Asks for a new backup folder and saves it. `null` if the user cancels. */
  backupChooseFolder: () => call<BackupStatus | null>("backup_choose_folder"),
  /** Goes back to the default backup folder. */
  backupUseDefaultFolder: () => call<BackupStatus>("backup_use_default_folder"),
  /**
   * Copies (or moves, with `removeOriginals`) the backups left in the previous folder to the
   * current one. An original is deleted only after its copy was verified.
   */
  backupMoveLeftBehind: (removeOriginals: boolean) =>
    call<RelocateOutcome>("backup_move_left_behind", { removeOriginals }),
  /** Replaces the current data with a backup, after a safety backup of the current data. */
  backupRestore: (name: string) => call<null>("backup_restore", { name }),
  /** Asks where to save the data file (`Gabaritei-<date>.gabaritei`) and writes it. `false` if cancelled. */
  exportData: () => call<boolean>("export_data"),
  /** Asks for a data file and replaces the local data with it, after a safety backup. `false` if cancelled. */
  importData: () => call<boolean>("import_data"),
  /** CLI paths, role configuration and RemNote (never the token). Writes defaults on first use. */
  aiSettings: () => call<AiSettings>("ai_settings"),
  /** Validates and saves the AI settings; returns them as saved. */
  saveAiSettings: (input: AiSettingsInput) => call<AiSettings>("save_ai_settings", { input }),
  /** Saves the RemNote token in the OS keychain (`null` or blank removes it). */
  setRemnoteToken: (token: string | null) => call<boolean>("set_remnote_token", { token }),
  /** Finds a CLI and reports its version and login state, using the saved path override. */
  cliTest: (agent: Agent) => call<CliStatus>("cli_test", { agent }),
  /** Checks the saved RemNote connection and lists the tools the server offers. */
  remnoteTest: () => call<RemNoteTest>("remnote_test"),
  /** Starts a generation in the background; progress arrives as `generation-status` events. */
  generationStart: (request: GenerateRequest) => call<null>("generation_start", { request }),
  /** Drafts waiting for review, newest first (counts and topics only). */
  draftsWaiting: () => call<DraftListItem[]>("drafts_waiting"),
  /** The spoiler-free summary of a draft: counts, problems and one state per question. */
  draftSummary: (id: string) => call<DraftSummary>("draft_summary", { id }),
  /** Imports a draft as a new generated collection: every question, or only the confirmed ones. */
  draftImport: (id: string, mode: ImportMode) => call<ImportResult>("draft_import", { id, mode }),
  /** Discards a draft that is waiting for review. */
  draftDiscard: (id: string) => call<null>("draft_discard", { id }),
  /** Asks which PDF, Markdown or text files to attach. Empty if the user cancels. */
  pickAttachments: () => call<string[]>("pick_attachments"),
  /** Calls `handler` on every generation status change. Returns a function that stops listening. */
  onGenerationStatus: async (handler: (status: GenerationStatus) => void) => {
    try {
      return await listen<GenerationStatus>("generation-status", (event) => handler(event.payload));
    } catch {
      return () => undefined;
    }
  },
  /** Asks the AI whether the key of a revealed question is right. Waits for the answer. */
  recheckAsk: (attemptId: string, position: number) =>
    call<RecheckView>("recheck_ask", { attemptId, position }),
  /** Applies the key the AI proposed (after the user confirmed); past attempts are re-scored. */
  recheckApply: (attemptId: string, position: number) =>
    call<AttemptView>("recheck_apply", { attemptId, position }),
  /** Drops the AI's proposal and keeps the current key. */
  recheckDismiss: (attemptId: string, position: number) =>
    call<null>("recheck_dismiss", { attemptId, position }),
  /** Writes the missing explanation of a revealed question with the AI. Waits for the answer. */
  explainAsk: (attemptId: string, position: number) =>
    call<AttemptView>("explain_ask", { attemptId, position }),
  /** Cancels the on-demand AI request that is running. */
  aiRequestCancel: () => call<boolean>("ai_request_cancel"),
  /** Runs the blind check again on a draft whose check failed (same background slot). */
  blindCheckStart: (draftId: string) => call<null>("blind_check_start", { draftId }),
  /** Stops the running generation. `false` if none was running. */
  generationCancel: () => call<boolean>("generation_cancel"),
  /** What the generation is doing now, or how the last one ended. */
  generationStatus: () => call<GenerationStatus>("generation_status"),
  /** Forgets how the last generation ended. */
  generationDismiss: () => call<null>("generation_dismiss"),
};
