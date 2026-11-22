import { beforeEach, describe, expect, it } from "vitest";
import type { GenerationKind } from "./bindings/GenerationKind";
import type { GenerationStage } from "./bindings/GenerationStage";
import type { GenerationStatus } from "./bindings/GenerationStatus";
import {
  applyGenerationStatus,
  cancelDiscardsDraft,
  generationStage,
  generationStageSentence,
  generationStages,
  resetGenerationWatch,
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
});
const idle: GenerationStatus = {
  running: false,
  startedAt: null,
  kind: null,
  stage: "preparing",
  progress: null,
  outcome: null,
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
