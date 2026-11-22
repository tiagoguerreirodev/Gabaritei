import { readFileSync } from "node:fs";
import { beforeEach, describe, expect, it } from "vitest";
import type { GenerationStatus } from "./bindings/GenerationStatus";
import {
  applyGenerationStatus,
  cancelDiscardsDraft,
  generationStage,
  generationStageSentence,
  generationStages,
  resetGenerationWatch,
  stageOfLabel,
} from "./generation";

const running = (label: string | null): GenerationStatus => ({
  running: true,
  startedAt: "2026-11-20T10:00:00.000Z",
  progress: label === null ? null : { kind: "thinking", label },
  outcome: null,
});
const idle: GenerationStatus = { running: false, startedAt: null, progress: null, outcome: null };

beforeEach(() => resetGenerationWatch());

describe("stageOfLabel", () => {
  it("maps the stage-starting labels and nothing else", () => {
    expect(stageOfLabel("Gerando as questões…")).toBe(0);
    expect(stageOfLabel("Conferindo às cegas…")).toBe(1);
    expect(stageOfLabel("Resolvendo as contestações…")).toBe(2);
    expect(stageOfLabel("Reconferindo as questões revisadas…")).toBe(2);
    expect(stageOfLabel("Lendo as fontes…")).toBeNull();
    expect(
      stageOfLabel("Os anexos originais não estão mais guardados; a contestação segue sem eles."),
    ).toBeNull();
  });

  it("knows every stage label the Rust blind check emits", () => {
    const source = readFileSync("src-tauri/src/ai/blind_check.rs", "utf8");
    const labels = [
      "Gerando as questões…",
      "Conferindo às cegas…",
      "Resolvendo as contestações…",
      "Reconferindo as questões revisadas…",
    ];
    for (const label of labels) {
      expect(source, `blind_check.rs no longer emits “${label}”`).toContain(`"${label}"`);
      expect(stageOfLabel(label), label).not.toBeNull();
    }
  });
});

describe("generation stage state", () => {
  it("follows a full run, never goes back and ignores unknown labels", () => {
    applyGenerationStatus(running("Gerando as questões…"));
    expect(generationStage()).toBe(0);
    expect(generationStages()).toHaveLength(3);
    applyGenerationStatus(running("Conferindo às cegas…"));
    expect(generationStage()).toBe(1);
    expect(generationStageSentence()).toBe("Etapa 2 de 3: Conferindo às cegas");
    applyGenerationStatus(running("Lendo as fontes…"));
    expect(generationStage()).toBe(1);
    applyGenerationStatus(running("Gerando as questões…"));
    expect(generationStage()).toBe(1);
    applyGenerationStatus(running(null));
    expect(generationStage()).toBe(1);
    applyGenerationStatus(running("Resolvendo as contestações…"));
    expect(generationStage()).toBe(2);
    expect(cancelDiscardsDraft()).toBe(true);
  });

  it("resets between runs", () => {
    applyGenerationStatus(running("Conferindo às cegas…"));
    applyGenerationStatus(running("Resolvendo as contestações…"));
    applyGenerationStatus(idle);
    expect(generationStage()).toBeNull();
    applyGenerationStatus(running("Gerando as questões…"));
    expect(generationStage()).toBe(0);
    expect(generationStages()).toHaveLength(3);
    expect(cancelDiscardsDraft()).toBe(false);
  });

  it("treats a run whose first stage label is the check as a re-check", () => {
    applyGenerationStatus(running("Conferindo às cegas…"));
    expect(generationStages().map((stage) => stage.id)).toEqual(["checking", "fixing"]);
    expect(generationStage()).toBe(0);
    expect(generationStageSentence()).toBe("Etapa 1 de 2: Conferindo às cegas");
    applyGenerationStatus(running("Reconferindo as questões revisadas…"));
    expect(generationStage()).toBe(1);
    expect(generationStageSentence()).toBe("Etapa 2 de 2: Resolvendo as contestações");
    expect(cancelDiscardsDraft()).toBe(false);
  });

  it("shows no stage when read mid-run with a label that is not a stage", () => {
    applyGenerationStatus(
      running("Os anexos originais não estão mais guardados; a contestação segue sem eles."),
    );
    expect(generationStage()).toBeNull();
    expect(generationStageSentence()).toBe("");
    applyGenerationStatus(running(null));
    expect(generationStage()).toBeNull();
  });
});
