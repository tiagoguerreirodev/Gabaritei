import { HashRouter, Route } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { beforeEach, describe, expect, it } from "vitest";
import type { DraftSummary } from "../bindings/DraftSummary";
import type { GenerationStatus } from "../bindings/GenerationStatus";
import { Toaster } from "../components/toasts";
import { applyGenerationStatus, resetGenerationWatch } from "../generation";
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
  confirmed: 3,
  needsCheck: false,
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
      case "draft_import_all":
        return {
          kind: "imported",
          result: { collectionId: "c9", title: "Coleção nova", imported: 3, merged: 1, skipped: 0 },
        };
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
  resetGenerationWatch();
});

/** The list item of question `position` in the package preview. */
function rowOf(position: number): HTMLElement {
  return screen.getByText(`Q${position}`).closest("li")!;
}

describe("Revisar pacote", () => {
  describe("when a new check reports progress before the previous finish", () => {
    const checking: GenerationStatus = {
      revision: 2,
      runRevision: 1,
      running: true,
      startedAt: summary.createdAt,
      kind: "recheck",
      stage: "checking",
      stages: [],
      canCancel: true,
      cancelDiscardsDraft: false,
      progress: null,
      outcome: null,
      outcomeKind: null,
    };

    function reportNextCheck() {
      applyGenerationStatus({ ...checking, revision: 5, runRevision: 4 });
      applyGenerationStatus({
        ...checking,
        revision: 3,
        runRevision: 3,
        running: false,
        startedAt: null,
        kind: null,
        stage: "preparing",
        canCancel: false,
        outcome: { kind: "draft", draftId: summary.id },
        outcomeKind: "recheck",
      });
    }

    it("refreshes the open package with the completed check's verdicts", async () => {
      let current: DraftSummary = {
        ...summary,
        checking: true,
        lines: [{ position: 1, state: "unchecked", hasProblem: false, content: null }],
      };
      mockIPC((command) => {
        if (command === "draft_summary") return current;
        throw new Error(`Unexpected command: ${command}`);
      });
      applyGenerationStatus(checking);
      renderAt("/revisar-pacote?id=d1");
      expect(await screen.findByText(/ainda está em andamento/)).toBeVisible();

      current = summary;
      reportNextCheck();

      await waitFor(() => expect(rowOf(1)).toHaveTextContent("Aprovada"));
      expect(screen.getByRole("button", { name: "Importar tudo" })).toBeEnabled();
    });

    it("refreshes checking indicators for packages in the waiting list", async () => {
      let isChecking = true;
      mockIPC((command) => {
        if (command !== "drafts_waiting") throw new Error(`Unexpected command: ${command}`);
        return [
          {
            id: summary.id,
            topic: summary.topic,
            count: summary.count,
            createdAt: summary.createdAt,
            disputed: summary.disputed,
            checking: isChecking,
          },
        ];
      });
      applyGenerationStatus(checking);
      renderAt("/revisar-pacote");
      expect(await screen.findByText(/conferindo…/)).toBeVisible();

      isChecking = false;
      reportNextCheck();

      await waitFor(() => expect(screen.queryByText(/conferindo…/)).not.toBeInTheDocument());
    });
  });

  async function openSummary() {
    backend();
    renderAt("/revisar-pacote?id=d1");

    await screen.findByRole("heading", { name: /Direito › Constitucional/ });
  }

  it("shows the package format and duplicate counts", async () => {
    await openSummary();
    expect(screen.getByText(/4 questões, 3 de múltipla escolha, 1 Certo\/Errado/)).toBeVisible();
    expect(screen.getByText(/1 já existem no banco/)).toBeVisible();
  });

  it("lists each question with its blind-check verdict", async () => {
    await openSummary();
    expect(screen.getByRole("list", { name: "Questões do pacote" })).toBeVisible();
    expect(rowOf(1)).toHaveTextContent("Aprovada");
    expect(rowOf(2)).toHaveTextContent("Contestada");
    expect(rowOf(3)).toHaveTextContent("Aprovada, corrigida");
    expect(rowOf(4)).toHaveTextContent("Aprovada");
  });

  it("shows the stem and options only for a flagged question", async () => {
    await openSummary();
    // The flagged question shows its stem and options, and only it.
    expect(screen.getByText("Enunciado da questão contestada?")).toBeInTheDocument();
    expect(screen.getByText("Alternativa dois")).toBeInTheDocument();
    expect(screen.getByText("B")).toBeInTheDocument();
    expect(document.querySelectorAll("details")).toHaveLength(1);
  });

  it("keeps answer keys, explanations and references off the preview", async () => {
    await openSummary();
    const text = document.body.textContent ?? "";
    for (const secret of ["Gabarito", "Explicação", "Referências", "answerId"]) {
      expect(text).not.toContain(secret);
    }
  });

  it("does not let the user answer a question in the preview", async () => {
    await openSummary();
    expect(document.querySelector("textarea, [role=radio], [role=radiogroup]")).toBeNull();
    expect(document.querySelector("input")).toBeNull();
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

    await screen.findByText("Q1");
    expect(rowOf(1)).toHaveTextContent("Com problemas");
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
    expect(calls.find((c) => c.command === "draft_import_all")?.args).toEqual({ id: "d1" });
    expect(await screen.findByText(/3 questões importadas para “Coleção nova”/)).toBeVisible();
    // The banner and the sidebar count are read again once the draft is gone.
    const commands = calls.map((c) => c.command);
    expect(commands.slice(commands.indexOf("draft_import_all"))).toContain("drafts_waiting");
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
    const tiles = region.getByRole("list", { name: "Contagens da conferência" });
    const tile = (label: string) => within(tiles).getByText(label).closest("li");
    expect(tile("Aprovadas")).toHaveTextContent("2");
    expect(tile("Aprovadas após correção")).toHaveTextContent("1");
    expect(tile("Contestadas")).toHaveTextContent("1");
    expect(tile("Com problemas")).toHaveTextContent("1");
    // It comes before the package details.
    const headings = screen.getAllByRole("heading", { level: 2 }).map((h) => h.textContent);
    expect(headings[0]).toBe("Resumo da conferência");
  });

  it("includes the verdict counts above the question lines", async () => {
    await openSummary();
    const checks = within(screen.getByRole("region", { name: "Conferência às cegas" }));
    expect(
      checks.getByText("2 aprovadas, 1 aprovadas após correção, 1 contestadas."),
    ).toBeVisible();
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
      expect(lines.getByText("Q1")).toBeVisible();

      await user.click(screen.getByRole("button", { name: /Contestadas 2/ }));
      expect(screen.getByRole("button", { name: /Contestadas 2/ })).toHaveAttribute(
        "aria-pressed",
        "true",
      );
      expect(lines.queryByText("Q1")).not.toBeInTheDocument();
      expect(lines.getByText("Q2")).toBeVisible();
      expect(lines.getByText("Q4")).toBeVisible();

      await user.click(screen.getByRole("button", { name: /Todas 4/ }));
      expect(lines.getByText("Q1")).toBeVisible();
    });

    it("an empty filter shows a message", async () => {
      backend(two);
      const user = userEvent.setup();
      renderAt("/revisar-pacote?id=d1");
      await user.click(await screen.findByRole("button", { name: /Com problemas 0/ }));
      expect(screen.getByText("Nenhuma questão neste filtro.")).toBeVisible();
      expect(screen.queryByRole("list", { name: "Questões do pacote" })).not.toBeInTheDocument();
    });

    async function openFlaggedSummary() {
      backend(two);
      const user = userEvent.setup();
      renderAt("/revisar-pacote?id=d1");
      await screen.findByText("Q2");
      return user;
    }

    it("opens every flagged question when asked to expand all", async () => {
      const user = await openFlaggedSummary();
      expect(document.querySelectorAll("details[open]")).toHaveLength(0);
      await user.click(screen.getByRole("button", { name: "Expandir todas" }));
      expect(document.querySelectorAll("details[open]")).toHaveLength(2);
      expect(screen.getByText("Opção A de Segundo enunciado")).toBeVisible();
    });

    it("closes every flagged question when asked to collapse all", async () => {
      const user = await openFlaggedSummary();
      await user.click(screen.getByRole("button", { name: "Expandir todas" }));
      await user.click(screen.getByRole("button", { name: "Recolher todas" }));
      expect(document.querySelectorAll("details[open]")).toHaveLength(0);
    });

    it("keeps keys and explanations hidden even when all flagged lines are open", async () => {
      const user = await openFlaggedSummary();
      await user.click(screen.getByRole("button", { name: "Expandir todas" }));
      expect(document.querySelectorAll("details[open]")).toHaveLength(2);
      for (const secret of ["Gabarito", "Explicação", "Referências", "answerId"]) {
        expect(document.body.textContent ?? "").not.toContain(secret);
      }
    });

    it("hides the expand button when nothing is flagged", async () => {
      backend({ ...summary, lines: summary.lines.map((line) => ({ ...line, content: null })) });
      renderAt("/revisar-pacote?id=d1");
      await screen.findByText("Q2");
      expect(screen.queryByRole("button", { name: "Expandir todas" })).not.toBeInTheDocument();
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
      needsCheck: true,
      confirmed: 0,
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
      needsCheck: true,
      confirmed: 0,
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
      needsCheck: true,
      confirmed: 0,
    });
    renderAt("/revisar-pacote?id=d1");
    expect(await screen.findByText(/ainda está em andamento/)).toBeVisible();
    for (const name of ["Conferir e importar tudo", "Importar só as confirmadas", "Descartar"]) {
      expect(screen.getByRole("button", { name })).toBeDisabled();
    }
    expect(screen.queryByRole("button", { name: "Conferir de novo" })).not.toBeInTheDocument();
    // Partial counts are not shown as if final.
    expect(screen.getAllByText(/Conferência em andamento/).length).toBeGreaterThan(0);
    expect(screen.queryByText(/0 aprovadas/)).not.toBeInTheDocument();
  });

  async function checkThenImport() {
    const calls = backend({ ...summary, needsCheck: true });
    const user = userEvent.setup();
    renderAt("/revisar-pacote?id=d1");
    await user.click(await screen.findByRole("button", { name: "Conferir e importar tudo" }));
    return calls;
  }

  it("opens the bank when the checked package has been imported", async () => {
    await checkThenImport();
    expect(await screen.findByRole("heading", { name: "Banco" })).toBeVisible();
  });

  it("submits one check-and-import request without polling", async () => {
    const calls = await checkThenImport();
    await screen.findByRole("heading", { name: "Banco" });
    expect(calls.filter((c) => c.command === "draft_import_all").map((c) => c.args)).toEqual([
      { id: "d1" },
    ]);
    const commands = calls.map((c) => c.command);
    expect(commands).not.toContain("blind_check_start");
    expect(commands).not.toContain("generation_status");
    expect(commands).not.toContain("draft_import");
  });

  it("shows a refused check request without promising automatic import", async () => {
    let refuse: (error: Error) => void = () => undefined;
    const response = new Promise<never>((_, reject) => (refuse = reject));
    mockIPC((command) => {
      switch (command) {
        case "draft_summary":
          return { ...summary, needsCheck: true };
        case "draft_import_all":
          return response;
        default:
          throw new Error(`Unexpected command: ${command}`);
      }
    });
    const user = userEvent.setup();
    renderAt("/revisar-pacote?id=d1");
    await user.click(await screen.findByRole("button", { name: "Conferir e importar tudo" }));
    expect(screen.getByText("Solicitando conferência e importação…")).toBeVisible();

    const busy = "Já existe uma geração em andamento. Espere terminar ou cancele.";
    refuse(new Error(busy));

    expect(await screen.findByText(busy)).toBeVisible();
    expect(screen.queryByText(/A importação segue sozinha/)).not.toBeInTheDocument();
    expect(screen.queryByText("Solicitando conferência e importação…")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Conferir e importar tudo" })).toBeEnabled();
  });

  it("refreshes the package after the backend reports a failed check", async () => {
    const failed: DraftSummary = {
      ...summary,
      needsCheck: true,
      checkFailed: "A conferência não respondeu todas as questões.",
    };
    const calls: string[] = [];
    mockIPC((command) => {
      calls.push(command);
      switch (command) {
        case "draft_summary":
          return failed;
        case "draft_import_all":
          return { kind: "pending", message: "A conferência não terminou. Nada foi importado." };
        default:
          throw new Error(`Unexpected command: ${command}`);
      }
    });
    const user = userEvent.setup();
    renderAt("/revisar-pacote?id=d1");
    await user.click(await screen.findByRole("button", { name: "Conferir e importar tudo" }));
    expect(
      await screen.findByText("A conferência não terminou. Nada foi importado."),
    ).toBeVisible();
    expect(calls).toEqual(["draft_summary", "draft_import_all", "draft_summary"]);
  });

  it("uses the backend's confirmed count to enable importing confirmed questions", async () => {
    backend({ ...summary, confirmed: 0 });
    renderAt("/revisar-pacote?id=d1");

    const button = await screen.findByRole("button", { name: "Importar só as confirmadas" });
    expect(button).toBeDisabled();
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
