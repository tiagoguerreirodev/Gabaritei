import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "./api";
import type { GenerationKind } from "./bindings/GenerationKind";
import type { GenerationStage } from "./bindings/GenerationStage";
import type { GenerationStatus } from "./bindings/GenerationStatus";
import {
  applyGenerationStatus,
  cancelledMessage,
  failedPrefix,
  generationStatus,
  cancelDiscardsDraft,
  generationStage,
  generationStageSentence,
  generationStages,
  resetGenerationWatch,
  watchGeneration,
} from "./generation";

const running = (
  kind: GenerationKind,
  stage: GenerationStage,
  label: string | null = null,
): GenerationStatus => ({
  running: true,
  startedAt: "2026-11-20T10:00:00.000Z",
  kind,
  stage,
  progress: label === null ? null : { kind: "thinking", label },
  outcome: null,
  outcomeKind: null,
});
const idle: GenerationStatus = {
  running: false,
  startedAt: null,
  kind: null,
  stage: "preparing",
  progress: null,
  outcome: null,
  outcomeKind: null,
};

beforeEach(() => resetGenerationWatch());

describe("generation stage state", () => {
  it("follows a full generation by the stage the backend reports", () => {
    applyGenerationStatus(running("generate", "preparing"));
    expect(generationStage()).toBeNull();
    expect(generationStageSentence()).toBe("");
    applyGenerationStatus(running("generate", "generating", "Gerando as questões…"));
    expect(generationStage()).toBe(0);
    expect(generationStages()).toHaveLength(3);
    expect(cancelDiscardsDraft()).toBe(false);
    applyGenerationStatus(running("generate", "checking", "Lendo as fontes…"));
    expect(generationStage()).toBe(1);
    expect(generationStageSentence()).toBe("Etapa 2 de 3: Conferindo às cegas");
    expect(cancelDiscardsDraft()).toBe(true);
    applyGenerationStatus(running("generate", "resolving"));
    expect(generationStage()).toBe(2);
    expect(cancelDiscardsDraft()).toBe(true);
  });

  it("shows the right stage when the page is read mid-run", () => {
    applyGenerationStatus(running("generate", "checking", "Raciocinando…"));
    expect(generationStage()).toBe(1);
    expect(generationStageSentence()).toBe("Etapa 2 de 3: Conferindo às cegas");
  });

  it("resets between runs", () => {
    applyGenerationStatus(running("generate", "resolving"));
    applyGenerationStatus(idle);
    expect(generationStage()).toBeNull();
    expect(cancelDiscardsDraft()).toBe(false);
    applyGenerationStatus(running("generate", "generating"));
    expect(generationStage()).toBe(0);
    expect(generationStages()).toHaveLength(3);
  });

  it("lists two stages for a re-check and never discards on cancel", () => {
    applyGenerationStatus(running("recheck", "preparing"));
    expect(generationStage()).toBeNull();
    applyGenerationStatus(running("recheck", "checking"));
    expect(generationStages().map((stage) => stage.id)).toEqual(["checking", "resolving"]);
    expect(generationStage()).toBe(0);
    expect(generationStageSentence()).toBe("Etapa 1 de 2: Conferindo às cegas");
    applyGenerationStatus(running("recheck", "resolving"));
    expect(generationStage()).toBe(1);
    expect(generationStageSentence()).toBe("Etapa 2 de 2: Resolvendo as contestações");
    expect(cancelDiscardsDraft()).toBe(false);
  });
});

describe("outcome wording", () => {
  const ended = (outcomeKind: GenerationKind | null): GenerationStatus => ({
    ...idle,
    outcome: { kind: "cancelled" },
    outcomeKind,
  });

  it("tells a cancelled re-check from a cancelled generation", () => {
    applyGenerationStatus(ended("recheck"));
    expect(cancelledMessage()).toBe("Conferência cancelada. O pacote foi mantido.");
    expect(cancelledMessage(true)).toBe("Conferência cancelada.");
    expect(failedPrefix()).toBe("A conferência falhou:");
    applyGenerationStatus(ended("generate"));
    expect(cancelledMessage()).toBe("Geração cancelada. Nada foi salvo.");
    expect(cancelledMessage(true)).toBe("Geração cancelada.");
    expect(failedPrefix()).toBe("A geração falhou:");
  });
});

describe("watchGeneration", () => {
  it("ignores the initial read when an event arrived first", async () => {
    let emit: (status: GenerationStatus) => void = () => undefined;
    let answer: (status: GenerationStatus) => void = () => undefined;
    vi.spyOn(api, "onGenerationStatus").mockImplementation((handler) => {
      emit = handler;
      return Promise.resolve(() => undefined);
    });
    vi.spyOn(api, "generationStatus").mockImplementation(
      () => new Promise<GenerationStatus>((resolve) => (answer = resolve)),
    );
    vi.spyOn(api, "draftsWaiting").mockResolvedValue([]);
    watchGeneration();
    await vi.waitFor(() => expect(api.generationStatus).toHaveBeenCalled());
    emit(running("generate", "checking"));
    answer(running("generate", "generating"));
    await Promise.resolve();
    await Promise.resolve();
    expect(generationStatus()?.stage).toBe("checking");
    vi.restoreAllMocks();
  });

  it("uses the initial read when no event came", async () => {
    vi.spyOn(api, "onGenerationStatus").mockResolvedValue(() => undefined);
    vi.spyOn(api, "generationStatus").mockResolvedValue(running("generate", "generating"));
    vi.spyOn(api, "draftsWaiting").mockResolvedValue([]);
    watchGeneration();
    await vi.waitFor(() => expect(generationStatus()?.stage).toBe("generating"));
    vi.restoreAllMocks();
  });
});
