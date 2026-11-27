import { HashRouter, Route } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { beforeEach, describe, expect, it } from "vitest";
import type { ActiveAttempt } from "../bindings/ActiveAttempt";
import type { HomeSummary } from "../bindings/HomeSummary";
import { emptyHome, stats } from "../test/fixtures";
import Home from "./Home";

const openAttempt: ActiveAttempt = {
  id: "att-7",
  title: "Direito › Constitucional › Controle",
  sourceKind: "topic",
  feedbackMode: "exam",
  startedAt: "2026-11-19T12:00:00.000Z",
  answered: 4,
  total: 10,
};

const summary: HomeSummary = {
  questions: 9368,
  overall: stats(30, 10, 10, 100),
  areas: [
    { name: "Português", stats: stats(2, 8, 0, 30) },
    { name: "Direito", stats: stats(28, 2, 10, 40) },
    { name: "Informática", stats: stats(0, 0, 0, 30) },
  ],
  weakestTopic: {
    topicId: 3,
    area: "Português",
    matter: "Gramática",
    topic: "Crase",
    stats: stats(1, 4, 0, 12),
    trend: [],
  },
  activeAttempts: [openAttempt],
  dueReviews: 3,
  dueInOpenAttempt: 2,
  draftsWaiting: 2,
  firstSteps: [],
};

function backend(home: HomeSummary | { error: string }) {
  mockIPC((command, args) => {
    if (command === "legacy_import_status") return { report: null, bankEmpty: false };
    if (command === "question_count_limits") return { min: 1, max: 12, initial: 10 };
    if (command === "session_start") {
      started.push(args);
      return { id: "att-new" };
    }
    if (command === "home") {
      if ("error" in home) {
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        return Promise.reject({ message: home.error });
      }
      return home;
    }
    throw new Error(`Unexpected command: ${command}`);
  });
}

let started: unknown[] = [];

function renderHome() {
  render(() => (
    <HashRouter>
      <Route path="/" component={Home} />
    </HashRouter>
  ));
}

describe("Início", () => {
  beforeEach(() => {
    started = [];
  });

  it("shows the summary strip, achievement per area and what waits", async () => {
    backend(summary);
    renderHome();

    const strip = await screen.findByRole("list", { name: "Resumo" });
    expect(within(strip).getByText("Aproveitamento").nextElementSibling).toHaveTextContent("60%");
    expect(within(strip).getByText("Cobertura").parentElement).toHaveTextContent(
      "Cobertura40%40 de 100",
    );
    expect(within(strip).getByText("Acerto entre respondidas").nextElementSibling).toHaveClass(
      "badge",
      "badge-good",
    );
    expect(screen.getByText("9.368 questões no banco.")).toBeVisible();

    const areas = within(
      screen.getByRole("heading", { name: "Aproveitamento por área" }).parentElement!,
    );
    const names = areas.getAllByRole("listitem").map((li) => li.textContent);
    expect(names).toEqual(["Português20%", "Direito70%"]);

    const waiting = screen.getByRole("heading", { name: "Aguardando você" }).parentElement!;
    expect(within(waiting).getByRole("link", { name: "Revisões pendentes" })).toHaveAttribute(
      "href",
      "#/banco?status=due,wrong&ordem=dueFirst",
    );
    expect(
      within(waiting).getByRole("link", { name: "Revisões pendentes" }).parentElement,
    ).toHaveTextContent("3 · 2 em tentativa aberta");
    expect(
      within(waiting).getByRole("link", { name: "Pacotes para revisar" }).parentElement,
    ).toHaveTextContent("2");
  });

  it("puts resuming the newest open attempt first and lists the others below", async () => {
    const older = { ...openAttempt, id: "att-5", title: "Caderno antigo" };
    backend({ ...summary, activeAttempts: [...summary.activeAttempts, older] });
    const user = userEvent.setup();
    renderHome();

    const today = within(await screen.findByRole("region", { name: openAttempt.title }));
    expect(today.getByText("Continue de onde parou")).toBeVisible();
    expect(today.getByText(/Simulado · 4 de 10 respondidas/)).toBeVisible();
    const others = within(screen.getByRole("region", { name: "Outras tentativas em andamento" }));
    expect(others.getByRole("button", { name: "Retomar Caderno antigo" })).toBeVisible();
    expect(others.queryByText(openAttempt.title)).toBeNull();

    await user.click(
      today.getByRole("button", { name: "Retomar Direito › Constitucional › Controle" }),
    );
    expect(window.location.hash).toBe("#/resolver?tentativa=att-7");
  });

  it("without an open attempt, offers the due reviews", async () => {
    backend({ ...summary, activeAttempts: [], dueInOpenAttempt: 0 });
    renderHome();

    const today = within(await screen.findByRole("region", { name: "3 revisões pendentes" }));
    expect(screen.queryByRole("region", { name: "Outras tentativas em andamento" })).toBeNull();
    expect(today.getByRole("link", { name: "Fazer revisões" })).toHaveAttribute(
      "href",
      "#/banco?status=due,wrong&ordem=dueFirst",
    );
  });

  it("with nothing open or due, trains the weakest topic", async () => {
    backend({ ...summary, activeAttempts: [], dueReviews: 0, dueInOpenAttempt: 0 });
    renderHome();

    const today = within(await screen.findByRole("region", { name: "Crase" }));
    expect(today.getByText("Português › Gramática · aproveitamento de 20%")).toBeVisible();
    expect(today.getByRole("link", { name: "Treinar assunto" })).toHaveAttribute(
      "href",
      "#/banco?assuntos=3",
    );
  });

  it("shows the first-use checklist above the main action, linking each step to do", async () => {
    backend({
      ...emptyHome,
      firstSteps: [
        { step: "checkAgents", done: true },
        { step: "generatePackage", done: true },
        { step: "reviewPackage", done: false },
        { step: "finishAttempt", done: false },
      ],
    });
    renderHome();

    const heading = await screen.findByRole("heading", { name: "Primeiros passos" });
    const checklist = within(heading.parentElement!);
    expect(checklist.getByText("2 de 4 concluídos")).toBeVisible();
    expect(checklist.getAllByRole("listitem").map((item) => item.textContent)).toEqual([
      "✓Confira os agentes (concluído)",
      "✓Gere o primeiro pacote (concluído)",
      "○Revise o pacoteRevisar pacote",
      "○Resolva a primeira tentativaIr para o Banco",
    ]);
    expect(checklist.getByRole("link", { name: "Revisar pacote" })).toHaveAttribute(
      "href",
      "#/revisar-pacote",
    );
    const mainAction = screen.getByRole("heading", { name: /nenhuma tentativa/ });
    expect(heading.compareDocumentPosition(mainAction)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
  });

  it("hides the first-use checklist once every step is done", async () => {
    backend(summary);
    renderHome();

    await screen.findByRole("list", { name: "Resumo" });
    expect(screen.queryByRole("heading", { name: "Primeiros passos" })).toBeNull();
  });

  it("hides what waits when nothing does", async () => {
    backend({ ...summary, dueReviews: 0, dueInOpenAttempt: 0, draftsWaiting: 0 });
    renderHome();

    await screen.findByRole("list", { name: "Resumo" });
    expect(screen.queryByRole("heading", { name: "Aguardando você" })).toBeNull();
  });

  it("shows only the waiting items with a count", async () => {
    backend({ ...summary, draftsWaiting: 0 });
    renderHome();

    const waiting = within(
      (await screen.findByRole("heading", { name: "Aguardando você" })).parentElement!,
    );
    expect(waiting.getByRole("link", { name: "Revisões pendentes" })).toBeVisible();
    expect(waiting.queryByRole("link", { name: "Pacotes para revisar" })).toBeNull();
  });

  it("refreshes after the first-run legacy import", async () => {
    let imported = false;
    mockIPC((command) => {
      if (command === "legacy_import_status") return { report: null, bankEmpty: !imported };
      if (command === "import_legacy_bank") {
        imported = true;
        return {
          report: {
            importedAt: "2026-11-19T12:00:00.000Z",
            questions: 9368,
            collections: 1,
            collectionItems: 1,
            topics: 1,
            images: 0,
          },
          alreadyImported: false,
        };
      }
      if (command === "home") return imported ? summary : emptyHome;
      throw new Error(`Unexpected command: ${command}`);
    });
    const user = userEvent.setup();
    render(() => (
      <HashRouter>
        <Route path="/" component={Home} />
      </HashRouter>
    ));

    expect(await screen.findByText("0 questões no banco.")).toBeVisible();
    await user.click(await screen.findByRole("button", { name: "Escolher pasta e importar" }));

    expect(await screen.findByText("9.368 questões no banco.")).toBeVisible();
    // The import card keeps its summary even though the bank is no longer empty.
    expect(screen.getByTestId("legacy-import-summary")).toBeVisible();
  });

  it("has pt-BR empty states for a fresh install", async () => {
    backend(emptyHome);
    render(() => (
      <HashRouter>
        <Route path="/" component={Home} />
      </HashRouter>
    ));

    const today = within(
      await screen.findByRole("region", { name: "Você ainda não finalizou nenhuma tentativa." }),
    );
    expect(today.getByRole("link", { name: "Ir para o Banco" })).toHaveAttribute("href", "#/banco");
    expect(screen.queryByRole("list", { name: "Resumo" })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Aguardando você" })).toBeNull();
  });

  it("shows the backend's message when loading fails", async () => {
    backend({ error: "Falha ao ler o banco." });
    render(() => (
      <HashRouter>
        <Route path="/" component={Home} />
      </HashRouter>
    ));
    expect(await screen.findByRole("alert")).toHaveTextContent("Falha ao ler o banco.");
  });

  it("retries a failed load: busy button, then data, no error and focus on the heading", async () => {
    let fail = true;
    let release: (() => void) | undefined;
    mockIPC((command) => {
      if (command === "legacy_import_status") return { report: null, bankEmpty: false };
      if (command === "home") {
        if (fail) {
          // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
          return Promise.reject({ message: "Falhou." });
        }
        return new Promise((resolve) => {
          release = () => resolve(summary);
        });
      }
      throw new Error(`Unexpected command: ${command}`);
    });
    const user = userEvent.setup();
    render(() => (
      <HashRouter>
        <Route path="/" component={Home} />
      </HashRouter>
    ));

    expect(await screen.findByRole("alert")).toHaveTextContent("Falhou.");
    fail = false;
    await user.click(screen.getByRole("button", { name: "Tentar novamente" }));
    expect(await screen.findByRole("button", { name: "Tentando…" })).toBeDisabled();
    await waitFor(() => expect(release).toBeDefined());
    release?.();

    expect(await screen.findByText("9.368 questões no banco.")).toBeVisible();
    expect(screen.queryByRole("alert")).toBeNull();
    await waitFor(() =>
      expect(screen.getByRole("heading", { level: 1, name: "Início" })).toHaveFocus(),
    );
  });
});
