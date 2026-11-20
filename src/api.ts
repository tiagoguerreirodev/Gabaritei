/**
 * The only place the frontend talks to the Rust backend.
 *
 * Types come from `src/bindings/`, which `cargo test` generates from the Rust structs
 * (ts-rs). Add one function per Tauri command here; never call `invoke` elsewhere.
 */
import { invoke, type InvokeArgs } from "@tauri-apps/api/core";
import type { Agent } from "./bindings/Agent";
import type { AiSettings } from "./bindings/AiSettings";
import type { AiSettingsInput } from "./bindings/AiSettingsInput";
import type { AppInfo } from "./bindings/AppInfo";
import type { AttemptView } from "./bindings/AttemptView";
import type { CliStatus } from "./bindings/CliStatus";
import type { CollectionDetail } from "./bindings/CollectionDetail";
import type { CollectionPage } from "./bindings/CollectionPage";
import type { CollectionQuery } from "./bindings/CollectionQuery";
import type { ErrorPayload } from "./bindings/ErrorPayload";
import type { HomeSummary } from "./bindings/HomeSummary";
import type { LegacyImportOutcome } from "./bindings/LegacyImportOutcome";
import type { LegacyImportStatus } from "./bindings/LegacyImportStatus";
import type { Performance } from "./bindings/Performance";
import type { PerformanceQuery } from "./bindings/PerformanceQuery";
import type { RemNoteTest } from "./bindings/RemNoteTest";
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
};
