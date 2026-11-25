import { mockIPC } from "@tauri-apps/api/mocks";
import { beforeEach, describe, expect, it } from "vitest";
import type { Catalog } from "./bindings/Catalog";
import { catalogState, loadCatalog, refreshCatalog, resetCatalog } from "./catalog";

const codexReady: Catalog = {
  agents: [
    {
      agent: "codex",
      status: { kind: "ready" },
      models: [
        { id: "m1", label: "M1", efforts: ["low", "high", "ultra"], supportsFast: true },
        { id: "m2", label: "M2", efforts: ["medium", "xhigh"], supportsFast: false },
        { id: "bare", label: "Bare", efforts: [], supportsFast: false },
      ],
    },
    {
      agent: "claude",
      status: { kind: "unavailable", reason: "O Claude Code não informa a lista." },
      models: [],
    },
  ],
};

beforeEach(() => resetCatalog());

describe("loadCatalog", () => {
  it("fetches the list once", async () => {
    let calls = 0;
    mockIPC((command) => {
      if (command === "ai_catalog") {
        calls += 1;
        return codexReady;
      }
    });
    await Promise.all([loadCatalog(), loadCatalog()]);
    await loadCatalog();
    expect(calls).toBe(1);
    expect(catalogState()).toEqual(codexReady);
  });

  it("shares one request between simultaneous refreshes", async () => {
    const refreshes: unknown[] = [];
    mockIPC((command, args) => {
      if (command === "ai_catalog") {
        refreshes.push(args);
        return codexReady;
      }
    });

    await loadCatalog();
    await Promise.all([refreshCatalog(), refreshCatalog()]);

    expect(refreshes).toEqual([{ refresh: false }, { refresh: true }]);
  });

  it("ignores the previous CLI path's list after a reset", async () => {
    let finishOld!: (catalog: Catalog) => void;
    const old = new Promise<Catalog>((resolve) => {
      finishOld = resolve;
    });
    const newCatalog: Catalog = { agents: [] };
    mockIPC((_command, args) => ((args as { refresh: boolean }).refresh ? newCatalog : old));

    const loading = loadCatalog();
    resetCatalog();
    await refreshCatalog();
    finishOld(codexReady);
    await loading;
    expect(catalogState()).toEqual(newCatalog);
  });

  it("records a failed request as null, not an exception", async () => {
    // The backend rejects with a plain error payload, not an Error instance.
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
    mockIPC(() => Promise.reject({ message: "falhou" }));
    await loadCatalog();
    expect(catalogState()).toBeNull();
  });
});
