import { mockIPC } from "@tauri-apps/api/mocks";
import { beforeEach, describe, expect, it } from "vitest";
import type { Catalog } from "./bindings/Catalog";
import type { RoleConfig } from "./bindings/RoleConfig";
import {
  catalogState,
  fallbackEfforts,
  loadCatalog,
  resetCatalog,
  roleForAgent,
  roleForModel,
  roleProblem,
} from "./catalog";

const codexReady: Catalog = {
  agents: [
    {
      agent: "codex",
      status: { kind: "ready" },
      models: [
        { id: "m1", label: "M1", efforts: ["low", "high", "ultra"] },
        { id: "m2", label: "M2", efforts: ["medium", "xhigh"] },
        { id: "bare", label: "Bare", efforts: [] },
      ],
    },
    {
      agent: "claude",
      status: { kind: "unavailable", reason: "O Claude Code não informa a lista." },
      models: [],
    },
  ],
};

const role = (agent: RoleConfig["agent"], model: string, effort: RoleConfig["effort"]) => ({
  agent,
  model,
  effort,
});

beforeEach(() => resetCatalog());

describe("roleProblem", () => {
  it("accepts a listed model and effort", () => {
    expect(roleProblem(codexReady, role("codex", "m1", "high"))).toBeUndefined();
  });

  it("flags a model the list no longer has", () => {
    expect(roleProblem(codexReady, role("codex", "gone", "low"))).toContain(
      "não está mais disponível",
    );
  });

  it("flags an effort the model does not support", () => {
    expect(roleProblem(codexReady, role("codex", "m2", "low"))).toContain("esforço");
  });

  it("accepts anything while the list is unavailable or not loaded", () => {
    expect(roleProblem(codexReady, role("claude", "qualquer", "max"))).toBeUndefined();
    expect(roleProblem(undefined, role("codex", "gone", "low"))).toBeUndefined();
    expect(roleProblem(null, role("codex", "gone", "low"))).toBeUndefined();
  });

  it("lets a model listed without efforts take any effort", () => {
    expect(roleProblem(codexReady, role("codex", "bare", "max"))).toBeUndefined();
  });
});

describe("roleForAgent", () => {
  it("picks the new agent's first listed model and a supported effort", () => {
    const next = roleForAgent(codexReady, "codex", role("claude", "sonnet", "high"));
    expect(next).toEqual({ agent: "codex", model: "m1", effort: "high" });
  });

  it("never keeps the old agent's model name when the new agent has no list", () => {
    const next = roleForAgent(codexReady, "claude", role("codex", "m1", "ultra"));
    expect(next.agent).toBe("claude");
    expect(next.model).toBe("");
    expect(next.effort).not.toBe("ultra");
  });
});

describe("roleForModel", () => {
  it("keeps the effort when the new model supports it", () => {
    expect(roleForModel(codexReady, role("codex", "m1", "high"), "m2").effort).toBe("xhigh");
    expect(roleForModel(codexReady, role("codex", "m2", "xhigh"), "m1").effort).toBe("high");
  });

  it("keeps the effort when the model is typed in free text", () => {
    expect(roleForModel(codexReady, role("claude", "a", "max"), "b")).toEqual(
      role("claude", "b", "max"),
    );
  });
});

describe("loadCatalog", () => {
  it("fetches once, and shares one request between simultaneous refreshes", async () => {
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

    await Promise.all([loadCatalog(true), loadCatalog(true)]);
    expect(calls).toBe(2);
  });

  it("records a failed request as null, not an exception", async () => {
    // The backend rejects with a plain error payload, not an Error instance.
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
    mockIPC(() => Promise.reject({ message: "falhou" }));
    await loadCatalog();
    expect(catalogState()).toBeNull();
  });
});

describe("fallbackEfforts", () => {
  it("offers ultra for Codex only", () => {
    expect(fallbackEfforts("codex")).toContain("ultra");
    expect(fallbackEfforts("claude")).not.toContain("ultra");
    expect(fallbackEfforts("claude")).toEqual(["low", "medium", "high", "xhigh", "max"]);
  });
});
