import { mockIPC } from "@tauri-apps/api/mocks";
import { describe, expect, it } from "vitest";
import { ApiError, api } from "./api";

describe("api", () => {
  it("calls the app_info command and returns its typed result", async () => {
    const calls: string[] = [];
    mockIPC((command) => {
      calls.push(command);
      return { name: "Gabaritei", version: "1.2.3" };
    });

    await expect(api.appInfo()).resolves.toEqual({ name: "Gabaritei", version: "1.2.3" });
    expect(calls).toEqual(["app_info"]);
  });

  it("turns the backend's error payload into an ApiError with its pt-BR message", async () => {
    // The backend rejects with a plain `ErrorPayload` object, not an Error instance.
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
    mockIPC(() => Promise.reject({ message: "Informe um assunto." }));

    const error: unknown = await api.appInfo().catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).message).toBe("Informe um assunto.");
  });

  it("uses a generic pt-BR message when the error has an unknown shape", async () => {
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
    mockIPC(() => Promise.reject("falha interna"));

    await expect(api.appInfo()).rejects.toThrow("Ocorreu um erro inesperado. Tente novamente.");
  });

  it("sends the include-blanks flag of the learning overview in camelCase", async () => {
    const sent: unknown[] = [];
    mockIPC((command, args) => {
      sent.push([command, args]);
      return { due: 0, upcoming: 0, errors: 0, practice: 0, practiceErrors: 0, items: [] };
    });

    await api.learningOverview(true);
    expect(sent).toEqual([["learning_overview", { includeBlanks: true }]]);
  });
});
