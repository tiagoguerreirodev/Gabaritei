import { HashRouter, Route } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { beforeEach, describe, expect, it } from "vitest";
import type { AiSettings } from "../bindings/AiSettings";
import type { Catalog } from "../bindings/Catalog";
import type { GenerateRequest } from "../bindings/GenerateRequest";
import type { GenerationStatus } from "../bindings/GenerationStatus";
import GenerationBanner from "../components/GenerationBanner";
import { Toaster } from "../components/toasts";
import { resetCatalog } from "../catalog";
import { applyGenerationStatus, resetGenerationWatch, watchGeneration } from "../generation";
import Generate from "./Generate";

const role = { agent: "codex", model: "gpt-6.2-sol", effort: "xhigh", fast: false } as const;
const settings: AiSettings = {
  cliPaths: { claude: null, codex: null },
  roles: {
    generate: role,
    blindCheck: { ...role, effort: "high", fast: false },
    recheck: role,
    explain: role,
  },
  remnote: { mode: "server", url: "https://r.example/mcp", tools: ["search"] },
  hasRemnoteToken: true,
};
const steps: GenerationStatus["stages"] = [
  { id: "generating", label: "Gerando as questões" },
  { id: "checking", label: "Conferindo às cegas" },
  { id: "resolving", label: "Resolvendo as contestações" },
];
const checkSteps = steps.slice(1);
const idle: GenerationStatus = {
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
const tree = {
  areas: [
    {
      name: "Direito",
      questions: 5,
      matters: [
        {
          name: "Constitucional",
          questions: 5,
          topics: [
            { id: 1, name: "Controle de constitucionalidade", questions: 5, accuracy: null },
          ],
        },
      ],
    },
  ],
  questions: 5,
};

const listed: Catalog = {
  agents: [
    {
      agent: "codex",
      status: { kind: "ready" },
      models: [
        { id: "gpt-6.2-sol", label: "GPT-6.2-Sol", efforts: ["high", "xhigh"], supportsFast: true },
        { id: "gpt-6-luna", label: "GPT-6-Luna", efforts: ["medium", "high"], supportsFast: false },
      ],
    },
    {
      agent: "claude",
      status: { kind: "unavailable", reason: "O Claude Code não informa a lista de modelos." },
      models: [],
    },
  ],
};

function backend(
  status: GenerationStatus = idle,
  aiSettings: AiSettings = settings,
  catalog?: Catalog,
  counts = { min: 1, max: 30, initial: 10 },
) {
  const calls: { command: string; args: Record<string, unknown> }[] = [];
  mockIPC((command, raw) => {
    const args = (raw ?? {}) as Record<string, unknown>;
    calls.push({ command, args });
    switch (command) {
      case "question_count_limits":
        return counts;
      case "topic_choices": {
        const path = args.path as { area: string; matter: string; topic: string };
        const match = ["Controle de constitucionalidade", "CONTROLE"].includes(path.topic);
        return {
          path,
          matters: ["Constitucional"],
          topics: ["Controle de constitucionalidade"],
          questions: match ? 5 : null,
          isNew: Boolean(path.area && path.matter && path.topic && !match),
        };
      }
      case "topics_tree":
        return tree;
      case "ai_catalog":
        if (!catalog) throw new Error("no catalog");
        return catalog;
      case "ai_settings":
        return aiSettings;
      case "generation_status":
        return status;
      case "generation_start":
      case "generation_dismiss":
        return null;
      case "generation_cancel":
        return true;
      case "pick_attachments":
        return ["/tmp/Lei 8.112.md"];
      default:
        throw new Error(`Unexpected command: ${command}`);
    }
  });
  return calls;
}

function renderPage(path = "/gerar") {
  window.location.hash = `#${path}`;
  resetGenerationWatch();
  render(() => (
    <>
      <HashRouter
        root={(props) => (
          <>
            <GenerationBanner />
            {props.children}
          </>
        )}
      >
        <Route path="/gerar" component={Generate} />
        <Route path="/outra" component={() => <h1>Outra página</h1>} />
      </HashRouter>
      <Toaster />
    </>
  ));
  watchGeneration();
}

async function fillTopic(user: ReturnType<typeof userEvent.setup>) {
  await user.type(await screen.findByLabelText("Área"), "Direito");
  await user.type(screen.getByLabelText("Matéria"), "Constitucional");
  await user.type(screen.getByLabelText("Assunto"), "Controle de constitucionalidade");
}

/** Opens the "Opções avançadas" disclosure (it starts collapsed). */
async function openAdvanced(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByText(/Opções avançadas/));
}

beforeEach(() => {
  window.location.hash = "";
  resetCatalog();
});

describe("Gerar", () => {
  it("prefills generator and checker from the role defaults", async () => {
    backend();
    renderPage();
    const generator = within(await screen.findByRole("group", { name: "Gerador" }));
    expect(generator.getByLabelText("Modelo: Gerador")).toHaveValue("gpt-6.2-sol");
    expect(generator.getByLabelText("Esforço: Gerador")).toHaveValue("xhigh");
    const checker = within(screen.getByRole("group", { name: "Conferente" }));
    expect(checker.getByLabelText("Esforço: Conferente")).toHaveValue("high");
  });

  it("starts a generation with what the form says", async () => {
    const calls = backend();
    const user = userEvent.setup();
    renderPage();
    await fillTopic(user);
    await user.clear(screen.getByLabelText("Quantidade"));
    await user.type(screen.getByLabelText("Quantidade"), "7");
    await user.type(screen.getByLabelText("Estilo da banca"), "Cebraspe");
    await user.selectOptions(screen.getByLabelText("Formato"), "boolean");
    await user.type(screen.getByLabelText("Instruções"), "Foque em jurisprudência");
    await user.type(screen.getByLabelText("Modelo: Conferente"), "-x");
    await user.click(screen.getByRole("button", { name: "Gerar" }));

    const start = await waitFor(() => {
      const call = calls.find((c) => c.command === "generation_start");
      expect(call).toBeDefined();
      return call!;
    });
    const request = start.args.request as GenerateRequest;
    expect(request).toMatchObject({
      topic: {
        area: "Direito",
        matter: "Constitucional",
        topic: "Controle de constitucionalidade",
      },
      count: 7,
      board: "Cebraspe",
      format: "boolean",
      instructions: "Foque em jurisprudência",
      sources: { remnote: false, web: false, attachments: [] },
      generator: { agent: "codex", model: "gpt-6.2-sol", effort: "xhigh", fast: false },
      checker: { agent: "codex", model: "gpt-6.2-sol-x", effort: "high", fast: false },
    });
  });

  it("starts a generation with Fast Mode on the chosen role", async () => {
    const calls = backend();
    const user = userEvent.setup();
    renderPage();
    await fillTopic(user);
    await user.click(await screen.findByRole("checkbox", { name: "Modo rápido: Gerador" }));
    await user.click(screen.getByRole("button", { name: "Gerar" }));
    const start = await waitFor(() => {
      const call = calls.find((c) => c.command === "generation_start");
      expect(call).toBeDefined();
      return call!;
    });
    const request = start.args.request as GenerateRequest;
    expect(request.generator?.fast).toBe(true);
    expect(request.checker?.fast).toBe(false);
  });

  it("uses the count limits supplied by the backend", async () => {
    backend(idle, settings, undefined, { min: 2, max: 7, initial: 4 });
    renderPage();
    const count = await screen.findByLabelText("Quantidade");
    expect(count).toHaveValue(4);
    expect(count).toHaveAttribute("min", "2");
    expect(count).toHaveAttribute("max", "7");
    expect(screen.getByText(/De 2 a 7/)).toBeVisible();
  });

  it("notes whether the topic is new or already has questions", async () => {
    backend();
    const user = userEvent.setup();
    renderPage();
    await fillTopic(user);
    expect(await screen.findByText(/Já existem 5 questões neste assunto/)).toBeVisible();
    await user.type(screen.getByLabelText("Assunto"), " novo");
    expect(await screen.findByText(/Assunto novo: será criado/)).toBeVisible();
  });

  it("renders an existing topic even when the returned name differs from the typed path", async () => {
    backend();
    const user = userEvent.setup();
    renderPage();
    await fillTopic(user);
    const topic = screen.getByLabelText("Assunto");
    await user.clear(topic);
    await user.type(topic, "CONTROLE");
    expect(await screen.findByText(/Já existem 5 questões/)).toBeVisible();
    expect(screen.queryByText(/Assunto novo:/)).toBeNull();
  });

  it("lets the web be combined with attachments and RemNote", async () => {
    const calls = backend();
    const user = userEvent.setup();
    renderPage();
    await fillTopic(user);

    await openAdvanced(user);
    await user.click(screen.getByRole("checkbox", { name: "Pesquisar na web" }));
    await user.click(screen.getByRole("checkbox", { name: "RemNote" }));
    await user.click(screen.getByRole("button", { name: "Anexar arquivos" }));
    expect(await screen.findByText("Lei 8.112.md")).toBeVisible();

    await user.click(screen.getByRole("button", { name: "Gerar" }));
    await waitFor(() => expect(calls.some((c) => c.command === "generation_start")).toBe(true));
    const request = calls.find((c) => c.command === "generation_start")!.args
      .request as GenerateRequest;
    expect(request.sources).toEqual({
      remnote: true,
      web: true,
      attachments: ["/tmp/Lei 8.112.md"],
    });
  });

  it("explains that RemNote is off in Configurações", async () => {
    backend(idle, { ...settings, remnote: { mode: "off", url: "", tools: [] } });
    renderPage();
    await openAdvanced(userEvent.setup());
    expect(await screen.findByRole("checkbox", { name: "RemNote" })).toBeDisabled();
    expect(screen.getByText(/ligue em Configurações/)).toBeVisible();
  });

  it("shows progress and a cancel button while a generation runs", async () => {
    const calls = backend({
      running: true,
      startedAt: "2026-11-20T10:00:00.000Z",
      kind: "generate",
      stage: "preparing",
      stages: steps,
      canCancel: true,
      cancelDiscardsDraft: false,
      progress: { kind: "reading", label: "Lendo as fontes…" },
      outcome: null,
      outcomeKind: null,
    });
    const user = userEvent.setup();
    renderPage();
    expect(await screen.findByRole("heading", { name: "Gerando o pacote…" })).toBeVisible();
    expect(screen.getAllByText(/Lendo as fontes…/).length).toBeGreaterThan(0);
    expect(screen.queryByRole("form", { name: "Nova geração" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancelar" }));
    expect(calls.some((c) => c.command === "generation_cancel")).toBe(true);
  });

  it("marks the current stage of a running generation", async () => {
    backend({
      running: true,
      startedAt: "2026-11-20T10:00:00.000Z",
      kind: "generate",
      stage: "resolving",
      stages: steps,
      canCancel: true,
      cancelDiscardsDraft: true,
      progress: { kind: "thinking", label: "Resolvendo as contestações…" },
      outcome: null,
      outcomeKind: null,
    });
    renderPage();
    const stages = within(await screen.findByRole("list", { name: "Etapas da geração" }));
    const items = stages.getAllByRole("listitem");
    expect(items).toHaveLength(3);
    expect(items[0]).not.toHaveAttribute("aria-current");
    expect(items[0]).toHaveTextContent("(concluída)");
    expect(items[2]).toHaveAttribute("aria-current", "step");
    expect(items[2]).toHaveTextContent("Resolvendo as contestações (em andamento)");
    expect(
      screen.getAllByRole("status").some((el) => /Etapa 3 de 3/.test(el.textContent ?? "")),
    ).toBe(true);
  });

  it("shows honest steps: pending until the user does something", async () => {
    backend();
    const user = userEvent.setup();
    renderPage();
    const steps = await screen.findByRole("list", { name: "Etapas" });
    const items = () => within(steps).getAllByRole("listitem");
    expect(items().map((li) => li.textContent)).toEqual([
      expect.stringContaining("Assunto"),
      expect.stringContaining("Opções"),
      expect.stringContaining("Revisão"),
    ]);
    expect(within(steps).queryByText(/Fontes/)).not.toBeInTheDocument();
    expect(within(steps).getByRole("listitem", { current: "step" })).toHaveTextContent("Assunto");
    expect(items()[1]).toHaveTextContent("(pendente)");
    expect(items()[2]).toHaveTextContent("(pendente)");

    await fillTopic(user);
    expect(items()[0]).toHaveTextContent("(concluída)");
    // Options stay pending: nothing was changed yet.
    expect(items()[1]).toHaveTextContent("(pendente)");
    expect(within(steps).queryByRole("listitem", { current: "step" })).toBeNull();

    await user.selectOptions(screen.getByLabelText("Formato"), "boolean");
    expect(items()[1]).toHaveTextContent("(concluída)");
    expect(items()[2]).toHaveTextContent("(pendente)");
  });

  it("marks Opções done once the advanced options are opened", async () => {
    backend();
    const user = userEvent.setup();
    renderPage();
    const steps = await screen.findByRole("list", { name: "Etapas" });
    await openAdvanced(user);
    expect(within(steps).getAllByRole("listitem")[1]).toHaveTextContent("(concluída)");
  });

  it("makes Revisão the current step while a package generates", async () => {
    backend({
      running: true,
      startedAt: "2026-11-20T10:00:00.000Z",
      kind: "generate",
      stage: "generating",
      stages: steps,
      canCancel: true,
      cancelDiscardsDraft: false,
      progress: { kind: "started", label: "Gerando as questões…" },
      outcome: null,
      outcomeKind: null,
    });
    renderPage();
    await waitFor(() => {
      const steps = screen.getByRole("list", { name: "Etapas" });
      expect(within(steps).getByRole("listitem", { current: "step" })).toHaveTextContent("Revisão");
    });
  });

  it("keeps the advanced options collapsed until opened", async () => {
    backend();
    const user = userEvent.setup();
    renderPage();
    const summary = await screen.findByText(/Opções avançadas/);
    const details = summary.closest("details")!;
    expect(details).not.toHaveAttribute("open");
    expect(screen.getByLabelText("Quantidade")).toBeVisible();
    expect(screen.getByLabelText("Quantidade")).toBeRequired();
    expect(screen.getByLabelText("Estilo da banca")).not.toBeVisible();
    await user.click(summary);
    expect(details).toHaveAttribute("open");
    expect(screen.getByLabelText("Estilo da banca")).toBeVisible();
  });

  it("explains in text what is missing, links it to the fields and focuses the first one", async () => {
    const calls = backend();
    const user = userEvent.setup();
    renderPage();
    await user.click(await screen.findByRole("button", { name: "Gerar" }));
    const message = await screen.findByText("Informe a área, a matéria e o assunto.");
    expect(message).toBeVisible();
    // Only the summary is an alert; the per-field text is not.
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    expect(screen.getByLabelText("Área")).toHaveFocus();
    expect(screen.getByLabelText("Área")).toBeInvalid();
    expect(screen.getByLabelText("Área")).toHaveAccessibleDescription(
      "Informe a área, a matéria e o assunto.",
    );
    expect(calls.some((c) => c.command === "generation_start")).toBe(false);

    // Typing does not make the message flicker; it is re-checked on submit or blur.
    await user.type(screen.getByLabelText("Área"), "Direito");
    expect(screen.getByText("Informe a área, a matéria e o assunto.")).toBeVisible();

    await fillTopic(user);
    await user.clear(screen.getByLabelText("Quantidade"));
    await user.click(screen.getByRole("button", { name: "Gerar" }));
    expect(
      await screen.findByText("A quantidade deve ser um número inteiro de 1 a 30."),
    ).toBeVisible();
    expect(screen.queryByText("Informe a área, a matéria e o assunto.")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Quantidade")).toBeInvalid();
    expect(screen.getByLabelText("Quantidade")).toHaveFocus();
    expect(screen.getByLabelText("Quantidade")).toHaveAccessibleDescription(
      /De 1 a 30\..*A quantidade deve ser um número inteiro de 1 a 30\./,
    );
    expect(calls.some((c) => c.command === "generation_start")).toBe(false);
  });

  it("opens the advanced options and says where to go when a model is blocking", async () => {
    const stale: AiSettings = {
      ...settings,
      roles: { ...settings.roles, generate: { ...role, model: "antigo" } },
    };
    backend(idle, stale, listed);
    renderPage();
    expect(await screen.findByText(/Corrija em “Opções avançadas”/)).toBeVisible();
    expect(screen.getByLabelText("Estilo da banca")).toBeVisible();
  });

  it("links to the finished package and lets the user dismiss the result", async () => {
    const calls = backend({
      ...idle,
      outcome: { kind: "draft", draftId: "d7" },
      outcomeKind: null,
    });
    renderPage();
    const link = await screen.findByRole("link", { name: "Revisar pacote" });
    expect(link.getAttribute("href")).toContain("/revisar-pacote?id=d7");
    expect(calls.length).toBeGreaterThan(0);
  });

  it("shows why a generation failed", async () => {
    backend({
      ...idle,
      outcome: { kind: "failed", message: "A IA devolveu 2 questões, mas você pediu 3." },
      outcomeKind: null,
    });
    renderPage();
    // Both the banner and the page's result card say it.
    const messages = await screen.findAllByText(/devolveu 2 questões, mas você pediu 3/);
    expect(messages.length).toBeGreaterThan(0);
    expect(messages[0]).toBeVisible();
  });
});

describe("Aviso de geração", () => {
  it("shows on every page that a package is being generated", async () => {
    backend({
      running: true,
      startedAt: "2026-11-20T10:00:00.000Z",
      kind: "generate",
      stage: "checking",
      stages: steps,
      canCancel: true,
      cancelDiscardsDraft: true,
      progress: { kind: "thinking", label: "Conferindo às cegas…" },
      outcome: null,
      outcomeKind: null,
    });
    renderPage("/outra");
    const banner = await screen.findByText(/Gerando um pacote…/);
    expect(banner).toBeVisible();
    expect(
      screen.getAllByRole("status").some((el) => /Conferindo às cegas/.test(el.textContent ?? "")),
    ).toBe(true);
    expect(screen.getByRole("link", { name: "Acompanhar" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Cancelar" })).toBeVisible();
  });

  it("renders stage labels and cancellation availability from the backend", async () => {
    backend({
      ...idle,
      running: true,
      kind: "recheck",
      stage: "checking",
      stages: [{ id: "checking", label: "Conferindo este pacote" }],
    });
    renderPage("/outra");

    expect(await screen.findByText(/Etapa 1 de 1: Conferindo este pacote/)).toBeVisible();
    expect(screen.queryByRole("button", { name: "Cancelar" })).not.toBeInTheDocument();
  });

  it("keeps the announced text to the stage, not the per-tool label", async () => {
    backend({
      running: true,
      startedAt: "2026-11-20T10:00:00.000Z",
      kind: "generate",
      stage: "generating",
      stages: steps,
      canCancel: true,
      cancelDiscardsDraft: false,
      progress: { kind: "started", label: "Gerando as questões…" },
      outcome: null,
      outcomeKind: null,
    });
    renderPage("/outra");
    await screen.findByText(/Gerando um pacote…/);
    applyGenerationStatus({
      running: true,
      startedAt: "2026-11-20T10:00:00.000Z",
      kind: "generate",
      stage: "generating",
      stages: steps,
      canCancel: true,
      cancelDiscardsDraft: false,
      progress: { kind: "reading", label: "Lendo a fonte 3…" },
      outcome: null,
      outcomeKind: null,
    });
    const live = screen.getByRole("status");
    expect(live).toHaveTextContent("Etapa 1 de 3: Gerando as questões");
    expect(live).not.toHaveTextContent("Lendo a fonte 3");
    expect(screen.getByText("Lendo a fonte 3…")).toBeVisible();
  });

  it("stays out of the way on the Gerar page, which shows the progress itself", async () => {
    backend({
      running: true,
      startedAt: "2026-11-20T10:00:00.000Z",
      kind: "generate",
      stage: "generating",
      stages: steps,
      canCancel: true,
      cancelDiscardsDraft: false,
      progress: { kind: "started", label: "Gerando as questões…" },
      outcome: null,
      outcomeKind: null,
    });
    renderPage();
    await screen.findByRole("heading", { name: "Gerando o pacote…" });
    expect(screen.queryByText(/Gerando um pacote…/)).not.toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Cancelar" })).toHaveLength(1);
  });

  it("keeps Cancelar in the banner while the Gerar page is loading or failed", async () => {
    const running: GenerationStatus = {
      running: true,
      startedAt: "2026-11-20T10:00:00.000Z",
      kind: "generate",
      stage: "generating",
      stages: steps,
      canCancel: true,
      cancelDiscardsDraft: false,
      progress: { kind: "started", label: "Gerando as questões…" },
      outcome: null,
      outcomeKind: null,
    };
    mockIPC((command) => {
      if (command === "generation_status") return running;
      if (command === "draft_waiting" || command === "drafts_waiting") return [];
      throw new Error("sem acesso");
    });
    renderPage();
    expect(await screen.findByText(/Gerando um pacote…/)).toBeVisible();
    expect(screen.getByRole("button", { name: "Cancelar" })).toBeVisible();
  });

  it("shows a pending state after Cancelar", async () => {
    const calls = backend({
      running: true,
      startedAt: "2026-11-20T10:00:00.000Z",
      kind: "generate",
      stage: "generating",
      stages: steps,
      canCancel: true,
      cancelDiscardsDraft: false,
      progress: { kind: "started", label: "Gerando as questões…" },
      outcome: null,
      outcomeKind: null,
    });
    const user = userEvent.setup();
    renderPage("/outra");
    await user.click(await screen.findByRole("button", { name: "Cancelar" }));
    expect(await screen.findByRole("button", { name: "Cancelando…" })).toBeDisabled();
    expect(calls.filter((c) => c.command === "generation_cancel")).toHaveLength(1);
  });

  it("asks before cancelling during the check, which discards the draft", async () => {
    const calls = backend({
      running: true,
      startedAt: "2026-11-20T10:00:00.000Z",
      kind: "generate",
      stage: "generating",
      stages: steps,
      canCancel: true,
      cancelDiscardsDraft: false,
      progress: { kind: "started", label: "Gerando as questões…" },
      outcome: null,
      outcomeKind: null,
    });
    const user = userEvent.setup();
    renderPage("/outra");
    await screen.findByText(/Gerando um pacote…/);
    applyGenerationStatus({
      running: true,
      startedAt: "2026-11-20T10:00:00.000Z",
      kind: "generate",
      stage: "checking",
      stages: steps,
      canCancel: true,
      cancelDiscardsDraft: true,
      progress: { kind: "thinking", label: "Conferindo às cegas…" },
      outcome: null,
      outcomeKind: null,
    });
    await user.click(screen.getByRole("button", { name: "Cancelar" }));
    const dialog = within(await screen.findByRole("dialog", { name: "Cancelar a geração?" }));
    expect(calls.some((c) => c.command === "generation_cancel")).toBe(false);
    await user.click(dialog.getByRole("button", { name: "Continuar gerando" }));
    expect(calls.some((c) => c.command === "generation_cancel")).toBe(false);
    await user.click(screen.getByRole("button", { name: "Cancelar" }));
    await user.click(
      within(await screen.findByRole("dialog")).getByRole("button", { name: "Cancelar geração" }),
    );
    await waitFor(() => expect(calls.some((c) => c.command === "generation_cancel")).toBe(true));
  });

  it("words a re-check as a check, and its cancel as keeping the package", async () => {
    backend({
      running: true,
      startedAt: "2026-11-20T10:00:00.000Z",
      kind: "recheck",
      stage: "checking",
      stages: checkSteps,
      canCancel: true,
      cancelDiscardsDraft: false,
      progress: { kind: "started", label: "Conferindo às cegas…" },
      outcome: null,
      outcomeKind: null,
    });
    renderPage();
    expect(await screen.findByRole("heading", { name: "Conferindo o pacote…" })).toBeVisible();
    expect(screen.queryByText(/Gerando/)).not.toBeInTheDocument();
    applyGenerationStatus({
      ...idle,
      outcome: { kind: "cancelled" },
      outcomeKind: "recheck",
    });
    expect(await screen.findByText("Conferência cancelada. O pacote foi mantido.")).toBeVisible();
    expect(screen.queryByText(/Nada foi salvo/)).not.toBeInTheDocument();
  });

  it("says a banner is checking, not generating, during a re-check", async () => {
    backend({
      running: true,
      startedAt: "2026-11-20T10:00:00.000Z",
      kind: "recheck",
      stage: "checking",
      stages: checkSteps,
      canCancel: true,
      cancelDiscardsDraft: false,
      progress: null,
      outcome: null,
      outcomeKind: null,
    });
    renderPage("/outra");
    expect(await screen.findByText(/Conferindo o pacote…/)).toBeVisible();
    expect(screen.queryByText(/Gerando um pacote/)).not.toBeInTheDocument();
  });

  it("does not ask when a re-check is cancelled, since the draft stays", async () => {
    const calls = backend({
      running: true,
      startedAt: "2026-11-20T10:00:00.000Z",
      kind: "recheck",
      stage: "checking",
      stages: checkSteps,
      canCancel: true,
      cancelDiscardsDraft: false,
      progress: { kind: "started", label: "Conferindo às cegas…" },
      outcome: null,
      outcomeKind: null,
    });
    const user = userEvent.setup();
    renderPage("/outra");
    await user.click(await screen.findByRole("button", { name: "Cancelar" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(calls.some((c) => c.command === "generation_cancel")).toBe(true);
  });

  it("offers the catalog's models and picks the new agent's first model when the agent changes", async () => {
    backend(idle, settings, listed);
    const user = userEvent.setup();
    renderPage();
    const generator = within(await screen.findByRole("group", { name: "Gerador" }));
    const model = await generator.findByRole("combobox", { name: "Modelo: Gerador" });
    expect(
      within(model)
        .getAllByRole("option")
        .map((o) => o.textContent),
    ).toEqual(["GPT-6.2-Sol", "GPT-6-Luna"]);

    // Claude offers no list: free text, and the Codex model name is not carried over.
    await user.selectOptions(generator.getByLabelText("Agente: Gerador"), "claude");
    expect(generator.getByRole("textbox", { name: "Modelo: Gerador" })).toHaveValue("");

    await user.selectOptions(generator.getByLabelText("Agente: Gerador"), "codex");
    expect(await generator.findByRole("combobox", { name: "Modelo: Gerador" })).toHaveValue(
      "gpt-6.2-sol",
    );
  });

  it("does not start a generation with a model the list no longer has", async () => {
    const stale: AiSettings = {
      ...settings,
      roles: { ...settings.roles, generate: { ...role, model: "antigo" } },
    };
    const calls = backend(idle, stale, listed);
    renderPage();
    expect(await screen.findByText(/“antigo” não está mais disponível/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Gerar" })).toBeDisabled();
    expect(calls.some((c) => c.command === "generation_start")).toBe(false);
  });
});
