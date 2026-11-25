import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "./api";
import type { DraftListItem } from "./bindings/DraftListItem";
import type { GenerationKind } from "./bindings/GenerationKind";
import type { GenerationStage } from "./bindings/GenerationStage";
import type { GenerationStatus } from "./bindings/GenerationStatus";
import {
  applyGenerationStatus,
  cancelledMessage,
  cancelledShortMessage,
  failedPrefix,
  generationStatus,
  cancelDiscardsDraft,
  generationStage,
  generationStageSentence,
  generationStages,
  refreshWaitingDrafts,
  resetGenerationWatch,
  watchGeneration,
  waitingDrafts,
} from "./generation";

const stages = [
  { id: "generating", label: "Gerando as questões" },
  { id: "checking", label: "Conferindo às cegas" },
  { id: "resolving", label: "Resolvendo as contestações" },
] as const;

let revision = 0;

const running = (
  kind: GenerationKind,
  stage: GenerationStage,
  label: string | null = null,
  runRevision = 1,
): GenerationStatus => ({
  revision: ++revision,
  runRevision,
  running: true,
  startedAt: "2026-11-20T10:00:00.000Z",
  kind,
  stage,
  stages: kind === "generate" ? [...stages] : stages.slice(1),
  canCancel: true,
  cancelDiscardsDraft: kind === "generate" && (stage === "checking" || stage === "resolving"),
  progress: label === null ? null : { kind: "thinking", label },
  outcome: null,
  outcomeKind: null,
});
const idle: GenerationStatus = {
  revision: 0,
  runRevision: 0,
  running: false,
  startedAt: null,
  kind: null,
  stage: "preparing",
  stages: [],
  canCancel: false,
  cancelDiscardsDraft: false,
  progress: null,
  outcome: null,
  outcomeKind: null,
};

const draft: DraftListItem = {
  id: "d1",
  topic: { area: "Direito", matter: "Constitucional", topic: "Controle" },
  count: 4,
  createdAt: "2026-11-20T10:00:00.000Z",
  disputed: 0,
  checking: false,
};

beforeEach(() => {
  revision = 0;
  resetGenerationWatch();
});

afterEach(() => vi.restoreAllMocks());

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
    applyGenerationStatus({ ...idle, revision: ++revision });
    expect(generationStage()).toBeNull();
    expect(cancelDiscardsDraft()).toBe(false);
    applyGenerationStatus(running("generate", "generating"));
    expect(generationStage()).toBe(0);
    expect(generationStages()).toHaveLength(3);
  });

  it("lists two stages for a re-check", () => {
    applyGenerationStatus(running("recheck", "preparing"));
    expect(generationStage()).toBeNull();
    applyGenerationStatus(running("recheck", "checking"));
    expect(generationStages().map((stage) => stage.id)).toEqual(["checking", "resolving"]);
    expect(generationStage()).toBe(0);
    expect(generationStageSentence()).toBe("Etapa 1 de 2: Conferindo às cegas");
    applyGenerationStatus(running("recheck", "resolving"));
    expect(generationStage()).toBe(1);
    expect(generationStageSentence()).toBe("Etapa 2 de 2: Resolvendo as contestações");
  });

  it("keeps the package when cancelling a re-check", () => {
    applyGenerationStatus(running("recheck", "resolving"));
    expect(cancelDiscardsDraft()).toBe(false);
  });
});

describe("outcome wording", () => {
  const ended = (outcomeKind: GenerationKind | null): GenerationStatus => ({
    ...idle,
    revision: ++revision,
    outcome: { kind: "cancelled" },
    outcomeKind,
  });

  it("explains that cancelling a re-check keeps the package", () => {
    applyGenerationStatus(ended("recheck"));
    expect(cancelledMessage()).toBe("Conferência cancelada. O pacote foi mantido.");
  });

  it("explains that cancelling a generation saves nothing", () => {
    applyGenerationStatus(ended("generate"));
    expect(cancelledMessage()).toBe("Geração cancelada. Nada foi salvo.");
  });

  it.each([
    ["recheck", "Conferência cancelada."],
    ["generate", "Geração cancelada."],
  ] as const)("uses a short cancellation message in the %s banner", (kind, message) => {
    applyGenerationStatus(ended(kind));
    expect(cancelledShortMessage()).toBe(message);
  });

  it.each([
    ["recheck", "A conferência falhou:"],
    ["generate", "A geração falhou:"],
  ] as const)("uses the %s failure prefix", (kind, message) => {
    applyGenerationStatus({ ...ended(kind), outcome: { kind: "failed", message: "Falhou." } });
    expect(failedPrefix()).toBe(message);
  });
});

describe("watchGeneration", () => {
  async function watchWithDeferredRead() {
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
    return { emit, answer };
  }

  it("ignores an initial read older than the event that arrived first", async () => {
    const { emit, answer } = await watchWithDeferredRead();
    const initial = running("generate", "generating");
    const checking = running("generate", "checking");
    emit(checking);
    answer(initial);
    await Promise.resolve();
    await Promise.resolve();
    expect(generationStatus()).toEqual(checking);
  });

  it("uses a newer initial read even after a completion event arrived", async () => {
    const { emit, answer } = await watchWithDeferredRead();
    const complete = { ...idle, revision: ++revision, runRevision: revision };
    const nextRun = running("recheck", "checking", null, revision + 1);
    emit(complete);
    vi.mocked(api.draftsWaiting).mockResolvedValue([draft]);
    answer(nextRun);
    await vi.waitFor(() => expect(generationStatus()).toEqual(nextRun));
    expect(waitingDrafts()).toEqual([draft]);
  });

  it("ignores a delayed completion after a newer run reports progress", async () => {
    const { emit, answer } = await watchWithDeferredRead();
    const complete = { ...idle, revision: ++revision, runRevision: revision };
    const nextRun = running("recheck", "checking", null, revision + 1);
    emit(nextRun);
    emit(complete);
    answer(complete);
    await Promise.resolve();
    await Promise.resolve();
    expect(generationStatus()).toEqual(nextRun);
  });

  it("refreshes waiting drafts when another run reports progress before the previous finish", async () => {
    const { emit, answer } = await watchWithDeferredRead();
    const first = running("generate", "checking");
    const complete = { ...idle, revision: ++revision, runRevision: revision };
    const newRunRevision = ++revision;
    const nextRun = running("generate", "generating", null, newRunRevision);
    emit(first);
    answer(first);
    expect(waitingDrafts()).toEqual([]);

    vi.mocked(api.draftsWaiting).mockResolvedValue([draft]);
    emit(nextRun);
    emit(complete);
    await vi.waitFor(() => expect(waitingDrafts()).toEqual([draft]));

    const reads = vi.mocked(api.draftsWaiting).mock.calls.length;
    emit({ ...nextRun, revision: ++revision, stage: "checking" });
    expect(api.draftsWaiting).toHaveBeenCalledTimes(reads);
  });

  it("reads waiting drafts after the listener is ready", async () => {
    let startListening: (unlisten: () => void) => void = () => undefined;
    vi.spyOn(api, "onGenerationStatus").mockImplementation(
      () => new Promise((resolve) => (startListening = resolve)),
    );
    vi.spyOn(api, "generationStatus").mockResolvedValue({
      ...idle,
      revision: 3,
      runRevision: 3,
      outcome: { kind: "draft", draftId: draft.id },
      outcomeKind: "generate",
    });
    let drafts: DraftListItem[] = [];
    vi.spyOn(api, "draftsWaiting").mockImplementation(() => Promise.resolve(drafts));
    watchGeneration();

    drafts = [draft];
    startListening(() => undefined);
    await vi.waitFor(() => expect(waitingDrafts()).toEqual([draft]));
  });

  it("ignores an older waiting drafts read that resolves after a refresh", async () => {
    let finishOldRead: (items: DraftListItem[]) => void = () => undefined;
    vi.spyOn(api, "draftsWaiting")
      .mockImplementationOnce(() => new Promise((resolve) => (finishOldRead = resolve)))
      .mockResolvedValue([draft]);
    const oldRead = refreshWaitingDrafts();
    await refreshWaitingDrafts();
    finishOldRead([]);
    await oldRead;

    expect(waitingDrafts()).toEqual([draft]);
  });

  it("uses the initial read when no event came", async () => {
    vi.spyOn(api, "onGenerationStatus").mockResolvedValue(() => undefined);
    vi.spyOn(api, "generationStatus").mockResolvedValue(running("generate", "generating"));
    vi.spyOn(api, "draftsWaiting").mockResolvedValue([]);
    watchGeneration();
    await vi.waitFor(() => expect(generationStatus()?.stage).toBe("generating"));
  });
});
