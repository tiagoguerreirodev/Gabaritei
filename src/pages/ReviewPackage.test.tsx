import { HashRouter, Route } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { beforeEach, describe, expect, it } from "vitest";
import type { DraftSummary } from "../bindings/DraftSummary";
import { Toaster } from "../components/toasts";
import ReviewPackage from "./ReviewPackage";

const summary: DraftSummary = {
  id: "d1",
  status: "review",
  createdAt: "2026-11-20T10:00:00.000Z",
  topic: { area: "Direito", matter: "Constitucional", topic: "Controle de constitucionalidade" },
  count: 4,
  multiple: 3,
  boolean: 1,
  requestedFormat: null,
  board: "Cebraspe",
  generator: { agent: "codex", model: "gpt-6.2-sol", effort: "xhigh" },
  checker: { agent: "codex", model: "gpt-6.2-sol", effort: "xhigh" },
  durationMs: 41_000,
  costUsd: null,
  lines: [
    { position: 1, state: "agreed", hasProblem: false, content: null },
    {
      position: 2,
      state: "disputed",
      hasProblem: false,
      content: {
        type: "multiple",
        stem: "Enunciado da questão contestada?",
        options: [
          { id: "a", text: "Alternativa um" },
          { id: "b", text: "Alternativa dois" },
        ],
      },
    },
    { position: 3, state: "fixed", hasProblem: false, content: null },
    { position: 4, state: "agreed", hasProblem: false, content: null },
  ],
  problems: [],
  duplicates: 1,
  agreed: 2,
  fixed: 1,
  disputed: 1,
  unchecked: 0,
  checkFailed: null,
  checking: false,
};

function backend(current: DraftSummary = summary) {
  const calls: { command: string; args: Record<string, unknown> }[] = [];
  mockIPC((command, raw) => {
    const args = (raw ?? {}) as Record<string, unknown>;
    calls.push({ command, args });
    switch (command) {
      case "draft_summary":
        return current;
      case "drafts_waiting":
        return [
          {
            id: "d1",
            topic: current.topic,
            count: current.count,
            createdAt: current.createdAt,
            disputed: 1,
            checking: false,
          },
        ];
      case "draft_import":
        return { collectionId: "c9", title: "Coleção nova", imported: 3, merged: 1, skipped: 0 };
      case "draft_discard":
        return null;
      case "blind_check_start":
        return null;
      case "collections":
        return { items: [], total: 0, page: 0, pageSize: 20, contests: [], boards: [] };
      case "topics_tree":
        return { areas: [], questions: 0 };
      default:
        throw new Error(`Unexpected command: ${command}`);
    }
  });
  return calls;
}

function renderAt(path: string) {
  window.location.hash = `#${path}`;
  render(() => (
    <>
      <HashRouter root={(props) => <>{props.children}</>}>
        <Route path="/revisar-pacote" component={ReviewPackage} />
        <Route path="/banco" component={() => <h1>Banco</h1>} />
        <Route path="/gerar" component={() => <h1>Gerar</h1>} />
      </HashRouter>
      <Toaster />
    </>
  ));
}

beforeEach(() => {
  window.location.hash = "";
});

describe("Revisar pacote", () => {
  it("shows counts and one line per question, with content only for the flagged one", async () => {
    backend();
    renderAt("/revisar-pacote?id=d1");

    await screen.findByRole("heading", { name: /Direito › Constitucional/ });
    expect(screen.getByText(/4 questões, 3 de múltipla escolha, 1 Certo\/Errado/)).toBeVisible();
    const lines = within(screen.getByRole("list", { name: "Questões do pacote" }));
    expect(lines.getByText("Q1 · conferida")).toBeVisible();
    expect(lines.getByText("Q2 · contestada")).toBeVisible();
    expect(lines.getByText("Q3 · corrigida e conferida")).toBeVisible();
    expect(screen.getByText(/1 já existem no banco/)).toBeVisible();

    // The flagged question shows its stem and options, and only it.
    expect(screen.getByText("Enunciado da questão contestada?")).toBeInTheDocument();
    expect(screen.getByText("Alternativa dois")).toBeInTheDocument();
    expect(screen.getByText("B")).toBeInTheDocument();
    expect(document.querySelectorAll("details")).toHaveLength(1);

    // No key, explanation or reference is rendered, and nothing can be answered here.
    const text = document.body.textContent ?? "";
    for (const secret of ["Gabarito", "Explicação", "Referências", "answerId"]) {
      expect(text).not.toContain(secret);
    }
    expect(document.querySelector("textarea, [role=radio], [role=radiogroup]")).toBeNull();
    const inputs = [...document.querySelectorAll("input")];
    expect(inputs).toHaveLength(1);
    expect(screen.getByRole("checkbox", { name: /Mostrar só as sinaladas/ })).toBe(inputs[0]);
  });

  it("explains why an agreed line expands and uses C/E for Certo/Errado", async () => {
    backend({
      ...summary,
      lines: [
        {
          position: 1,
          state: "agreed",
          hasProblem: true,
          content: {
            type: "boolean",
            stem: "Afirmação a julgar.",
            options: [
              { id: "c", text: "Certo" },
              { id: "e", text: "Errado" },
            ],
          },
        },
      ],
    });
    renderAt("/revisar-pacote?id=d1");

    expect(await screen.findByText("Q1 · conferida · com problema")).toBeVisible();
    expect(screen.getByText("C")).toBeInTheDocument();
    expect(screen.getByText("E")).toBeInTheDocument();
  });

  it("renders problems without any question content", async () => {
    backend({
      ...summary,
      problems: [
        { position: 2, message: "Questões de múltipla escolha têm 5 alternativas; esta tem 4." },
        { position: null, message: "Algo no pacote todo." },
      ],
    });
    renderAt("/revisar-pacote?id=d1");
    expect(await screen.findByText(/Q2 · Questões de múltipla escolha têm 5/)).toBeVisible();
    expect(screen.getByText("Algo no pacote todo.")).toBeVisible();
    expect(screen.getByRole("button", { name: "Importar tudo" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Importar só as confirmadas" })).toBeEnabled();
  });

  it("imports everything and goes to the Banco", async () => {
    const calls = backend();
    const user = userEvent.setup();
    renderAt("/revisar-pacote?id=d1");
    await user.click(await screen.findByRole("button", { name: "Importar tudo" }));

    await screen.findByRole("heading", { name: "Banco" });
    expect(calls.find((c) => c.command === "draft_import")?.args).toEqual({
      id: "d1",
      mode: "all",
    });
    expect(await screen.findByText(/3 questões importadas para “Coleção nova”/)).toBeVisible();
    // The banner and the sidebar count are read again once the draft is gone.
    const commands = calls.map((c) => c.command);
    expect(commands.slice(commands.indexOf("draft_import"))).toContain("drafts_waiting");
  });

  it("imports only the confirmed questions", async () => {
    const calls = backend();
    const user = userEvent.setup();
    renderAt("/revisar-pacote?id=d1");
    await user.click(await screen.findByRole("button", { name: "Importar só as confirmadas" }));
    await waitFor(() =>
      expect(calls.find((c) => c.command === "draft_import")?.args).toEqual({
        id: "d1",
        mode: "confirmed",
      }),
    );
  });

  it("puts the verdict counts at the top", async () => {
    backend({ ...summary, problems: [{ position: 2, message: "Algo errado." }] });
    renderAt("/revisar-pacote?id=d1");
    const region = within(await screen.findByRole("region", { name: "Resumo da conferência" }));
    expect(region.getByText("Conferidas").closest("div")).toHaveTextContent("2");
    expect(region.getByText("Corrigidas e conferidas").closest("div")).toHaveTextContent("1");
    expect(region.getByText("Contestadas").closest("div")).toHaveTextContent("1");
    expect(region.getByText("Problemas").closest("div")).toHaveTextContent("1");
    // It comes before the package details.
    const headings = screen.getAllByRole("heading", { level: 2 }).map((h) => h.textContent);
    expect(headings[0]).toBe("Resumo da conferência");
  });

  describe("curating flagged questions", () => {
    const flaggedLine = (position: number, stem: string) => ({
      position,
      state: "disputed" as const,
      hasProblem: false,
      content: {
        type: "multiple" as const,
        stem,
        options: [
          { id: "a", text: `Opção A de ${stem}` },
          { id: "b", text: `Opção B de ${stem}` },
        ],
      },
    });
    const two: DraftSummary = {
      ...summary,
      disputed: 2,
      lines: [
        { position: 1, state: "agreed", hasProblem: false, content: null },
        flaggedLine(2, "Primeiro enunciado"),
        { position: 3, state: "agreed", hasProblem: false, content: null },
        flaggedLine(4, "Segundo enunciado"),
      ],
    };

    it("filters to the flagged lines only", async () => {
      backend(two);
      const user = userEvent.setup();
      renderAt("/revisar-pacote?id=d1");
      const lines = within(await screen.findByRole("list", { name: "Questões do pacote" }));
      expect(lines.getAllByRole("listitem", { hidden: true }).length).toBeGreaterThanOrEqual(4);
      expect(lines.getByText("Q1 · conferida")).toBeVisible();

      await user.click(screen.getByRole("checkbox", { name: /Mostrar só as sinaladas \(2\)/ }));
      expect(lines.queryByText("Q1 · conferida")).not.toBeInTheDocument();
      expect(lines.getByText("Q2 · contestada")).toBeVisible();
      expect(lines.getByText("Q4 · contestada")).toBeVisible();

      await user.click(screen.getByRole("checkbox", { name: /Mostrar só as sinaladas/ }));
      expect(lines.getByText("Q1 · conferida")).toBeVisible();
    });

    it("expands and collapses every flagged line, never showing key or explanation", async () => {
      backend(two);
      const user = userEvent.setup();
      renderAt("/revisar-pacote?id=d1");
      await screen.findByText("Q2 · contestada");
      const open = () => document.querySelectorAll("details[open]").length;
      expect(open()).toBe(0);

      await user.click(screen.getByRole("button", { name: "Expandir todas" }));
      expect(open()).toBe(2);
      expect(screen.getByText("Opção A de Segundo enunciado")).toBeVisible();
      for (const secret of ["Gabarito", "Explicação", "Referências", "answerId"]) {
        expect(document.body.textContent ?? "").not.toContain(secret);
      }

      await user.click(screen.getByRole("button", { name: "Recolher todas" }));
      expect(open()).toBe(0);
    });

    it("hides the toolbar when nothing is flagged", async () => {
      backend({ ...summary, lines: summary.lines.map((line) => ({ ...line, content: null })) });
      renderAt("/revisar-pacote?id=d1");
      await screen.findByText("Q2 · contestada");
      expect(screen.queryByRole("button", { name: "Expandir todas" })).not.toBeInTheDocument();
      expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    });
  });

  it("asks before discarding, and keeps the package when the user goes back", async () => {
    const calls = backend();
    const user = userEvent.setup();
    renderAt("/revisar-pacote?id=d1");
    await user.click(await screen.findByRole("button", { name: "Descartar" }));
    const dialog = within(await screen.findByRole("dialog", { name: "Descartar o pacote?" }));
    await user.click(dialog.getByRole("button", { name: "Voltar" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(calls.some((c) => c.command === "draft_discard")).toBe(false);
  });

  it("discards the package", async () => {
    const calls = backend();
    const user = userEvent.setup();
    renderAt("/revisar-pacote?id=d1");
    await user.click(await screen.findByRole("button", { name: "Descartar" }));
    const dialog = within(await screen.findByRole("dialog", { name: "Descartar o pacote?" }));
    expect(calls.some((c) => c.command === "draft_discard")).toBe(false);
    await user.click(dialog.getByRole("button", { name: "Descartar pacote" }));
    await screen.findByRole("heading", { name: "Gerar" });
    expect(calls.some((c) => c.command === "draft_discard")).toBe(true);
    const commands = calls.map((c) => c.command);
    expect(commands.slice(commands.indexOf("draft_discard"))).toContain("drafts_waiting");
  });

  it("says when the blind check failed and offers to run it again", async () => {
    const calls = backend({
      ...summary,
      lines: summary.lines.map((line) => ({ ...line, state: "unchecked" as const })),
      agreed: 0,
      fixed: 0,
      disputed: 0,
      unchecked: 4,
      checkFailed: "A conferência não respondeu todas as questões.",
    });
    const user = userEvent.setup();
    renderAt("/revisar-pacote?id=d1");
    expect(await screen.findByText(/não foi concluída: A conferência não respondeu/)).toBeVisible();
    expect(screen.getByRole("button", { name: "Importar só as confirmadas" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Conferir de novo" }));
    await waitFor(() => expect(calls.some((c) => c.command === "blind_check_start")).toBe(true));
  });

  it("only suggests importing the confirmed ones when there are some", async () => {
    backend({
      ...summary,
      lines: summary.lines.map((line) => ({ ...line, state: "unchecked" as const })),
      agreed: 0,
      fixed: 0,
      disputed: 0,
      unchecked: 4,
    });
    const user = userEvent.setup();
    renderAt("/revisar-pacote?id=d1");
    await user.click(await screen.findByRole("button", { name: "Descartar" }));
    const dialog = within(await screen.findByRole("dialog", { name: "Descartar o pacote?" }));
    expect(dialog.queryByText(/Importar só as confirmadas/)).not.toBeInTheDocument();
  });

  it("keeps every action off while the blind check is still running", async () => {
    backend({
      ...summary,
      checking: true,
      lines: summary.lines.map((line) => ({ ...line, state: "unchecked" as const })),
      agreed: 0,
      fixed: 0,
      disputed: 0,
      unchecked: 4,
    });
    renderAt("/revisar-pacote?id=d1");
    expect(await screen.findByText(/ainda está em andamento/)).toBeVisible();
    for (const name of ["Conferir e importar tudo", "Importar só as confirmadas", "Descartar"]) {
      expect(screen.getByRole("button", { name })).toBeDisabled();
    }
    expect(screen.queryByRole("button", { name: "Conferir de novo" })).not.toBeInTheDocument();
    // Partial counts are not shown as if final.
    expect(screen.getAllByText(/Conferência em andamento/).length).toBeGreaterThan(0);
    expect(screen.queryByText(/0 conferidas/)).not.toBeInTheDocument();
  });

  it("checks the unchecked questions before importing everything", async () => {
    const unchecked: DraftSummary = {
      ...summary,
      lines: summary.lines.map((line) => ({ ...line, state: "unchecked" as const })),
      agreed: 0,
      fixed: 0,
      disputed: 0,
      unchecked: 4,
    };
    let checked = false;
    const calls: string[] = [];
    mockIPC((command) => {
      calls.push(command);
      switch (command) {
        case "draft_summary":
          return checked ? summary : unchecked;
        case "blind_check_start":
          checked = true;
          return null;
        case "generation_status":
          return { running: false, startedAt: null, progress: null, outcome: null };
        case "generation_dismiss":
          return null;
        case "draft_import":
          return { collectionId: "c9", title: "Coleção nova", imported: 4, merged: 0, skipped: 0 };
        case "collections":
          return { items: [], total: 0, page: 0, pageSize: 20, contests: [], boards: [] };
        case "topics_tree":
          return { areas: [], questions: 0 };
        default:
          throw new Error(`Unexpected command: ${command}`);
      }
    });
    const user = userEvent.setup();
    renderAt("/revisar-pacote?id=d1");
    await user.click(await screen.findByRole("button", { name: "Conferir e importar tudo" }));

    await screen.findByRole("heading", { name: "Banco" });
    // The check runs first, then the import.
    expect(calls.indexOf("blind_check_start")).toBeGreaterThan(-1);
    expect(calls.indexOf("draft_import")).toBeGreaterThan(calls.indexOf("blind_check_start"));
  });

  it("imports nothing when the check of the unchecked questions fails again", async () => {
    const failed: DraftSummary = {
      ...summary,
      lines: summary.lines.map((line) => ({ ...line, state: "unchecked" as const })),
      agreed: 0,
      fixed: 0,
      disputed: 0,
      unchecked: 4,
      checkFailed: "A conferência não respondeu todas as questões.",
    };
    const calls: string[] = [];
    mockIPC((command) => {
      calls.push(command);
      switch (command) {
        case "draft_summary":
          return failed;
        case "blind_check_start":
        case "generation_dismiss":
          return null;
        case "generation_status":
          return { running: false, startedAt: null, progress: null, outcome: null };
        default:
          throw new Error(`Unexpected command: ${command}`);
      }
    });
    const user = userEvent.setup();
    renderAt("/revisar-pacote?id=d1");
    await user.click(await screen.findByRole("button", { name: "Conferir e importar tudo" }));
    expect(await screen.findByText(/Nada foi importado/)).toBeVisible();
    expect(calls).not.toContain("draft_import");
  });

  it("lists the packages waiting when no package is chosen", async () => {
    backend();
    renderAt("/revisar-pacote");
    const link = await screen.findByRole("link", {
      name: "Direito › Constitucional › Controle de constitucionalidade",
    });
    expect(link).toHaveAttribute("href", "#/revisar-pacote?id=d1");
    expect(screen.getByText(/1 contestadas/)).toBeVisible();
  });

  it("shows a package that was already imported without actions", async () => {
    backend({ ...summary, status: "imported" });
    renderAt("/revisar-pacote?id=d1");
    expect(await screen.findByText(/já foi importado/)).toBeVisible();
    expect(screen.queryByRole("button", { name: "Importar tudo" })).not.toBeInTheDocument();
  });
});
