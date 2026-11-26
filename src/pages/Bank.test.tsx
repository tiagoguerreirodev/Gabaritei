import { HashRouter, Route } from "@solidjs/router";
import { render, screen, waitFor } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { beforeEach, describe, expect, it } from "vitest";
import type { QuestionFilter } from "../bindings/QuestionFilter";
import type { SelectionSummary } from "../bindings/SelectionSummary";
import type { TopicTree } from "../bindings/TopicTree";
import { summary, tree } from "./bank/bankFixtures";
import Bank from "./Bank";

type Handlers = {
  summaryFor?: (filter: QuestionFilter) => Promise<SelectionSummary> | SelectionSummary;
};

function backend(options: Handlers & { tree?: TopicTree; failSummary?: boolean } = {}) {
  const filters: QuestionFilter[] = [];
  mockIPC((command, args) => {
    if (command === "topics_tree") return options.tree ?? tree;
    if (command === "selection_summary") {
      const filter = (args as { filter: QuestionFilter }).filter;
      filters.push(filter);
      // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
      if (options.failSummary) return Promise.reject({ message: "Falhou ao contar." });
      return options.summaryFor ? options.summaryFor(filter) : summary;
    }
    if (command === "question_count_limits") return { min: 1, max: 140, initial: 20 };
  });
  return filters;
}

const renderBank = () =>
  render(() => (
    <HashRouter>
      <Route path="/" component={Bank} />
    </HashRouter>
  ));

beforeEach(() => {
  window.location.hash = "#/";
});

describe("Banco", () => {
  it("restores_filters_from_the_url", async () => {
    window.location.hash = "#/?status=wrong&assuntos=2&ordem=random&modo=exam";
    const filters = backend();
    renderBank();
    await screen.findByText("140 questões");
    expect(filters.at(-1)).toEqual({
      topicIds: [2],
      statuses: ["wrong"],
      collectionId: null,
      attemptId: null,
    });
    expect(screen.getByRole("checkbox", { name: /^Erradas/ })).toBeChecked();
    expect(screen.getByRole("radio", { name: "Simulado" })).toBeChecked();
  });

  it("checking_a_status_updates_the_url_and_the_summary", async () => {
    const filters = backend();
    renderBank();
    await userEvent.click(await screen.findByRole("checkbox", { name: /^Erradas/ }));
    await waitFor(() => expect(filters.at(-1)?.statuses).toEqual(["wrong"]));
    expect(window.location.hash).toContain("status=wrong");
  });

  it("a_preset_sets_filters_and_order", async () => {
    const filters = backend();
    renderBank();
    const preset = await screen.findByRole("button", { name: "Revisões de hoje" });
    await userEvent.click(preset);
    await waitFor(() => expect(filters.at(-1)?.statuses).toEqual(["due", "wrong"]));
    expect(window.location.hash).toContain("ordem=dueFirst");
    expect(preset).toHaveAttribute("aria-pressed", "true");
  });

  it("clearing_filters_resets_the_url", async () => {
    window.location.hash = "#/?status=wrong";
    backend();
    renderBank();
    await userEvent.click(await screen.findByRole("button", { name: "Limpar filtros" }));
    await waitFor(() => expect(window.location.hash).not.toContain("status"));
  });

  it("shows_the_latest_summary_when_replies_arrive_out_of_order", async () => {
    let releaseFirst: (value: SelectionSummary) => void = () => {};
    backend({
      summaryFor: (filter) =>
        filter.statuses.length === 0
          ? new Promise<SelectionSummary>((resolve) => (releaseFirst = resolve))
          : { ...summary, matching: 22 },
    });
    renderBank();
    await userEvent.click(await screen.findByRole("checkbox", { name: /^Erradas/ }));
    await screen.findByText("22 questões");
    releaseFirst({ ...summary, matching: 140 });
    await Promise.resolve();
    expect(screen.getByText("22 questões")).toBeInTheDocument();
    expect(screen.queryByText("140 questões")).not.toBeInTheDocument();
  });

  it("shows_a_retry_when_the_summary_fails", async () => {
    backend({ failSummary: true });
    renderBank();
    expect(await screen.findByText("Falhou ao contar.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Tentar novamente" })).toBeInTheDocument();
  });

  it("offers_to_clear_filters_when_nothing_matches", async () => {
    window.location.hash = "#/?status=blank";
    backend({ summaryFor: () => ({ ...summary, matching: 0 }) });
    renderBank();
    expect(await screen.findByText("Nenhuma questão com estes filtros.")).toBeInTheDocument();
  });

  it("shows_the_import_hint_on_an_empty_bank", async () => {
    backend({ tree: { areas: [], questions: 0 } });
    renderBank();
    expect(await screen.findByText("Nenhuma questão no banco ainda.")).toBeInTheDocument();
  });
});
