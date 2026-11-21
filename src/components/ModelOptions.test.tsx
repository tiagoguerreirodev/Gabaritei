import { render, screen, waitFor, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { createSignal } from "solid-js";
import { beforeEach, describe, expect, it } from "vitest";
import type { Catalog } from "../bindings/Catalog";
import type { RoleConfig } from "../bindings/RoleConfig";
import { resetCatalog } from "../catalog";
import { CatalogNotice, EffortField, ModelField } from "./ModelOptions";

const catalog: Catalog = {
  agents: [
    {
      agent: "codex",
      status: { kind: "ready" },
      models: [
        { id: "m1", label: "Modelo Um", efforts: ["low", "high"] },
        { id: "m2", label: "Modelo Dois", efforts: ["medium", "xhigh", "ultra"] },
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
  return (
    <>
      <ModelField label="Gerar" value={value()} onChange={setValue} />
      <EffortField label="Gerar" value={value()} onChange={setValue} />
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
  mockIPC((command) => (command === "ai_catalog" ? catalog : undefined));
});

describe("ModelField and EffortField", () => {
  it("lists the models and the efforts of the selected model", async () => {
    render(() => <Harness initial={{ agent: "codex", model: "m1", effort: "low" }} />);
    const model = await screen.findByRole("combobox", { name: "Modelo: Gerar" });
    expect(optionNames(model)).toEqual(["Modelo Um", "Modelo Dois"]);
    const effort = screen.getByRole("combobox", { name: "Esforço: Gerar" });
    expect(optionNames(effort)).toEqual(["Baixo", "Alto"]);
  });

  it("changes the effort options, and a kept-or-nearest effort, when the model changes", async () => {
    const user = userEvent.setup();
    render(() => <Harness initial={{ agent: "codex", model: "m1", effort: "high" }} />);
    const model = await screen.findByRole("combobox", { name: "Modelo: Gerar" });
    await user.selectOptions(model, "m2");
    const effort = screen.getByRole<HTMLSelectElement>("combobox", { name: "Esforço: Gerar" });
    expect(optionNames(effort)).toEqual(["Médio", "Extra alto", "Ultra"]);
    expect(effort.value).toBe("xhigh");
  });

  it("falls back to free text with a warning when the agent has no list", async () => {
    render(() => <Harness initial={{ agent: "claude", model: "sonnet", effort: "high" }} />);
    const model = await screen.findByRole("textbox", { name: "Modelo: Gerar" });
    expect((model as HTMLInputElement).value).toBe("sonnet");
    expect(await screen.findByText(/não informa a lista de modelos/)).toBeTruthy();
    const effort = screen.getByRole("combobox", { name: "Esforço: Gerar" });
    expect(optionNames(effort)).not.toContain("Ultra");
  });

  it("falls back to free text when the catalog request fails", async () => {
    // The backend rejects with a plain error payload, not an Error instance.
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
    mockIPC(() => Promise.reject({ message: "falhou" }));
    render(() => <Harness initial={{ agent: "codex", model: "m1", effort: "high" }} />);
    expect(await screen.findByRole("textbox", { name: "Modelo: Gerar" })).toBeTruthy();
    expect(await screen.findByText(/Não foi possível carregar as listas/)).toBeTruthy();
  });

  it("shows a saved model the list no longer has as unavailable and keeps it selected", async () => {
    render(() => <Harness initial={{ agent: "codex", model: "antigo", effort: "high" }} />);
    const model = await screen.findByRole<HTMLSelectElement>("combobox", { name: "Modelo: Gerar" });
    await waitFor(() => expect(optionNames(model)).toContain("antigo (não disponível)"));
    expect(model.value).toBe("antigo");
  });
});
