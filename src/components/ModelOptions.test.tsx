import { render, screen, waitFor, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { createSignal } from "solid-js";
import { beforeEach, describe, expect, it } from "vitest";
import type { Catalog } from "../bindings/Catalog";
import type { RoleConfig } from "../bindings/RoleConfig";
import type { RoleOptions } from "../bindings/RoleOptions";
import { createRoleEditor, resetCatalog } from "../catalog";
import { CatalogNotice, EffortField, FastField, ModelField } from "./ModelOptions";

const catalog: Catalog = {
  agents: [
    {
      agent: "codex",
      status: { kind: "ready" },
      models: [
        { id: "m1", label: "Modelo Um", efforts: ["low", "high"], supportsFast: true },
        {
          id: "m2",
          label: "Modelo Dois",
          efforts: ["medium", "xhigh", "ultra"],
          supportsFast: false,
        },
      ],
    },
    {
      agent: "claude",
      status: { kind: "unavailable", reason: "O Claude Code não informa a lista de modelos." },
      models: [],
    },
  ],
};

function Harness(props: { initial: RoleConfig }) {
  const [value, setValue] = createSignal(props.initial);
  const editor = createRoleEditor(value, setValue);
  return (
    <>
      <ModelField label="Gerar" editor={editor} />
      <EffortField label="Gerar" editor={editor} />
      <FastField label="Gerar" editor={editor} />
      <CatalogNotice agents={[value().agent]} />
    </>
  );
}

const optionNames = (select: HTMLElement) =>
  within(select)
    .getAllByRole("option")
    .map((option) => option.textContent);

beforeEach(() => {
  resetCatalog();
  mockIPC((command, args) => {
    if (command === "ai_catalog") return catalog;
    const { config } = args as { config: RoleConfig };
    if (command === "ai_change_role") {
      expect(args).toMatchObject({ change: { kind: "model", model: "m2" } });
      return { ...config, model: "m2", effort: "xhigh", fast: false };
    }
    const codexOptions: Record<string, RoleOptions> = {
      m1: { efforts: ["low", "high"], showFast: true, problem: null },
      m2: { efforts: ["medium", "xhigh", "ultra"], showFast: false, problem: null },
    };
    const freeText: RoleOptions = {
      efforts: ["low", "medium", "high", "xhigh", "max"],
      showFast: true,
      problem: null,
    };
    return config.agent === "codex" ? (codexOptions[config.model] ?? freeText) : freeText;
  });
});

describe("ModelField and EffortField", () => {
  it("lists the models and the efforts of the selected model", async () => {
    render(() => <Harness initial={{ agent: "codex", model: "m1", effort: "low", fast: false }} />);
    const model = await screen.findByRole("combobox", { name: "Modelo: Gerar" });
    expect(optionNames(model)).toEqual(["Modelo Um", "Modelo Dois"]);
    const effort = screen.getByRole("combobox", { name: "Esforço: Gerar" });
    await waitFor(() => expect(optionNames(effort)).toEqual(["Baixo", "Alto"]));
  });

  it("renders the effort selected by the backend after a model switch", async () => {
    const user = userEvent.setup();
    render(() => (
      <Harness initial={{ agent: "codex", model: "m1", effort: "high", fast: false }} />
    ));
    const model = await screen.findByRole("combobox", { name: "Modelo: Gerar" });
    await user.selectOptions(model, "m2");
    const effort = screen.getByRole<HTMLSelectElement>("combobox", { name: "Esforço: Gerar" });
    await waitFor(() => expect(effort.value).toBe("xhigh"));
  });

  it("renders the backend's effort options after a model switch", async () => {
    const user = userEvent.setup();
    render(() => (
      <Harness initial={{ agent: "codex", model: "m1", effort: "high", fast: false }} />
    ));
    const model = await screen.findByRole("combobox", { name: "Modelo: Gerar" });
    await user.selectOptions(model, "m2");
    const effort = screen.getByRole("combobox", { name: "Esforço: Gerar" });
    await waitFor(() => expect(optionNames(effort)).toEqual(["Médio", "Extra alto", "Ultra"]));
  });

  it("falls back to free text with a warning when the agent has no list", async () => {
    render(() => (
      <Harness initial={{ agent: "claude", model: "sonnet", effort: "high", fast: false }} />
    ));
    const model = await screen.findByRole("textbox", { name: "Modelo: Gerar" });
    expect((model as HTMLInputElement).value).toBe("sonnet");
    expect(await screen.findByText(/não informa a lista de modelos/)).toBeTruthy();
  });

  it("renders the backend fallback efforts when the agent cannot list models", async () => {
    render(() => (
      <Harness initial={{ agent: "claude", model: "sonnet", effort: "high", fast: false }} />
    ));
    const effort = await screen.findByRole("combobox", { name: "Esforço: Gerar" });
    await waitFor(() =>
      expect(optionNames(effort)).toEqual(["Baixo", "Médio", "Alto", "Extra alto", "Máximo"]),
    );
  });

  it("falls back to free text when the catalog request fails", async () => {
    // The backend rejects with a plain error payload, not an Error instance.
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
    mockIPC(() => Promise.reject({ message: "falhou" }));
    render(() => (
      <Harness initial={{ agent: "codex", model: "m1", effort: "high", fast: false }} />
    ));
    expect(await screen.findByRole("textbox", { name: "Modelo: Gerar" })).toBeTruthy();
    expect(await screen.findByText(/Não foi possível carregar as listas/)).toBeTruthy();
  });

  it("shows a saved model the list no longer has as unavailable and keeps it selected", async () => {
    render(() => (
      <Harness initial={{ agent: "codex", model: "antigo", effort: "high", fast: false }} />
    ));
    const model = await screen.findByRole<HTMLSelectElement>("combobox", { name: "Modelo: Gerar" });
    await waitFor(() => expect(optionNames(model)).toContain("antigo (não disponível)"));
    expect(model.value).toBe("antigo");
  });

  it("shows the checkbox only where Fast Mode can apply", async () => {
    const user = userEvent.setup();
    render(() => <Harness initial={{ agent: "codex", model: "m1", effort: "low", fast: false }} />);
    const model = await screen.findByRole("combobox", { name: "Modelo: Gerar" });
    expect(screen.getByRole("checkbox", { name: "Modo rápido: Gerar" })).toBeTruthy();
    await user.selectOptions(model, "m2");
    await waitFor(() =>
      expect(screen.queryByRole("checkbox", { name: "Modo rápido: Gerar" })).toBeNull(),
    );
  });

  it("shows the checkbox when the list is unavailable, with the extra usage note for Claude", async () => {
    render(() => (
      <Harness initial={{ agent: "claude", model: "opus", effort: "low", fast: false }} />
    ));
    expect(await screen.findByRole("checkbox", { name: "Modo rápido: Gerar" })).toBeTruthy();
    expect(screen.getByText(/uso extra/)).toBeTruthy();
  });
});
