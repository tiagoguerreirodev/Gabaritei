/**
 * The only place the frontend talks to the Rust backend.
 *
 * Types come from `src/bindings/`, which `cargo test` generates from the Rust structs
 * (ts-rs). Add one function per Tauri command here; never call `invoke` elsewhere.
 */
import { invoke, type InvokeArgs } from "@tauri-apps/api/core";
import type { AppInfo } from "./bindings/AppInfo";
import type { ErrorPayload } from "./bindings/ErrorPayload";

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
};
