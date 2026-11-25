import { HashRouter, Route, A } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { beforeEach, describe, expect, it } from "vitest";
import type { DraftListItem } from "../bindings/DraftListItem";
import type { GenerationStatus } from "../bindings/GenerationStatus";
import { refreshWaitingDrafts, resetGenerationWatch } from "../generation";
import Layout from "./Layout";

const topic = { area: "Direito", matter: "Constitucional", topic: "Controle" };
const draft = (id: string): DraftListItem => ({
  id,
  topic,
  count: 4,
  createdAt: "2026-11-20T10:00:00.000Z",
  disputed: 0,
  checking: false,
});
// The outcome was already dismissed (as clicking "Revisar pacote" used to do).
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

function backend(waiting: () => DraftListItem[], current: GenerationStatus = idle) {
  const calls: string[] = [];
  mockIPC((command) => {
    calls.push(command);
    switch (command) {
      case "generation_status":
        return current;
      case "drafts_waiting":
        return waiting();
      case "generation_dismiss":
        return null;
      case "app_info":
        return { name: "Gabaritei", version: "0.2.0" };
      default:
        throw new Error(`Unexpected command: ${command}`);
    }
  });
  return calls;
}

function renderApp() {
  window.location.hash = "#/";
  resetGenerationWatch();
  render(() => (
    <HashRouter root={Layout}>
      <Route path="/" component={() => <A href="/outra">Ir</A>} />
      <Route path="/outra" component={() => <A href="/">Voltar</A>} />
      <Route path="/revisar-pacote" component={() => <h1>Revisar pacote</h1>} />
    </HashRouter>
  ));
}

beforeEach(() => {
  window.location.hash = "";
});

describe("Pacotes esperando revisão", () => {
  it("keeps the banner on every page until the drafts are imported or discarded", async () => {
    const user = userEvent.setup();
    let waiting = [draft("d1")];
    const calls = backend(() => waiting);
    renderApp();

    const banner = await screen.findByRole("status");
    expect(banner).toHaveTextContent("1 pacote esperando revisão");
    const link = within(banner).getByRole("link", { name: "Revisar pacote" });
    expect(link.getAttribute("href")).toContain("/revisar-pacote?id=d1");

    // Opening the review must not consume the notice, and neither must leaving the page.
    await user.click(link);
    expect(await screen.findByRole("heading", { name: "Revisar pacote" })).toBeVisible();
    expect(screen.getByRole("status")).toHaveTextContent("1 pacote esperando revisão");
    expect(calls).not.toContain("generation_dismiss");
    window.location.hash = "#/outra";
    expect(await screen.findByRole("status")).toHaveTextContent("1 pacote esperando revisão");

    // Importing or discarding it removes the notice.
    waiting = [];
    await refreshWaitingDrafts();
    await waitFor(() => expect(screen.queryByRole("status")).not.toBeInTheDocument());
  });

  it("shows a single notice, with no empty dismiss banner, after a successful generation", async () => {
    backend(() => [draft("d1")], {
      ...idle,
      outcome: { kind: "draft", draftId: "d1" },
    });
    renderApp();
    await screen.findByText("1 pacote esperando revisão.");
    expect(screen.getAllByRole("status")).toHaveLength(1);
    expect(screen.queryByRole("button", { name: "Dispensar" })).not.toBeInTheDocument();
  });

  it("links to the list when more than one package is waiting", async () => {
    backend(() => [draft("d1"), draft("d2")]);
    renderApp();
    const banner = await screen.findByRole("status");
    expect(banner).toHaveTextContent("2 pacotes esperando revisão");
    expect(
      within(banner).getByRole("link", { name: "Revisar pacotes" }).getAttribute("href"),
    ).toMatch(/#\/revisar-pacote$/);
  });

  it("shows the sidebar entry with a count only while something is waiting", async () => {
    let waiting = [draft("d1"), draft("d2")];
    backend(() => waiting);
    renderApp();
    const menu = within(screen.getByRole("navigation", { name: "Menu principal" }));
    const entry = await menu.findByRole("link", { name: /Revisar pacote/ });
    expect(entry).toHaveTextContent("2");
    expect(entry.getAttribute("href")).toMatch(/#\/revisar-pacote$/);

    waiting = [];
    await refreshWaitingDrafts();
    await waitFor(() =>
      expect(menu.queryByRole("link", { name: /Revisar pacote/ })).not.toBeInTheDocument(),
    );
  });

  it("shows nothing when no package is waiting", async () => {
    backend(() => []);
    renderApp();
    await screen.findByRole("link", { name: "Início" });
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: /Revisar pacote/ })).not.toBeInTheDocument();
  });
});
