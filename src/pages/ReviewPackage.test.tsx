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
    { position: 1, state: "agreed" },
    { position: 2, state: "disputed" },
    { position: 3, state: "fixed" },
    { position: 4, state: "agreed" },
  ],
  problems: [],
  duplicates: 1,
  agreed: 2,
  fixed: 1,
  disputed: 1,
  unchecked: 0,
  checkFailed: null,
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
  it("shows counts and one line per question, and nothing from the questions", async () => {
    backend();
    renderAt("/revisar-pacote?id=d1");

    await screen.findByRole("heading", { name: /Direito › Constitucional/ });
    expect(screen.getByText(/4 questões, 3 de múltipla escolha, 1 Certo\/Errado/)).toBeVisible();
    const lines = within(screen.getByRole("list", { name: "Questões do pacote" }));
    expect(lines.getByText("Q1 · conferida")).toBeVisible();
    expect(lines.getByText("Q2 · contestada")).toBeVisible();
    expect(lines.getByText("Q3 · corrigida e conferida")).toBeVisible();
    expect(screen.getByText(/1 já existem no banco/)).toBeVisible();

    // No stem, option, key or explanation is rendered, and the page never asks for them.
    const text = document.body.textContent ?? "";
    for (const secret of ["Enunciado", "Alternativa", "Gabarito:", "Explicação", "answerId"]) {
      expect(text).not.toContain(secret);
    }
    expect(document.querySelector("ol, textarea, [role=radio], [role=radiogroup]")).toBeNull();
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

  it("discards the package", async () => {
    const calls = backend();
    const user = userEvent.setup();
    renderAt("/revisar-pacote?id=d1");
    await user.click(await screen.findByRole("button", { name: "Descartar" }));
    await screen.findByRole("heading", { name: "Gerar" });
    expect(calls.some((c) => c.command === "draft_discard")).toBe(true);
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
