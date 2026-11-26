import { render, screen, waitFor } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SelectionSummary } from "../../bindings/SelectionSummary";
import ResultPanel from "./ResultPanel";
import { summary } from "./bankFixtures";
import { emptyFilter } from "./filterUrl";

function backend(options: { openAttempt?: string; startError?: string } = {}) {
  const started: unknown[] = [];
  mockIPC((command, args) => {
    if (command === "question_count_limits") {
      const { available } = (args as { context: { available: number } }).context;
      return { min: 1, max: available, initial: Math.min(20, available) };
    }
    if (command === "collection") {
      const finished = "2026-11-01T00:00:00.000Z";
      return {
        id: "c-1",
        attempts: [
          { id: "old", finishedAt: finished },
          ...(options.openAttempt ? [{ id: options.openAttempt, finishedAt: null }] : []),
        ],
      };
    }
    if (command === "session_start") {
      started.push((args as { request: unknown }).request);
      // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
      if (options.startError) return Promise.reject({ message: options.startError });
      return { id: "att-1" };
    }
  });
  return started;
}

const renderPanel = (overrides: { summary?: SelectionSummary; filter?: typeof emptyFilter } = {}) =>
  render(() => (
    <ResultPanel
      summary={overrides.summary ?? summary}
      filter={overrides.filter ?? emptyFilter}
      order="random"
      mode="study"
      onOrder={vi.fn()}
      onMode={vi.fn()}
    />
  ));

beforeEach(() => {
  window.location.hash = "";
});

describe("ResultPanel", () => {
  it("shows_counts_without_question_text", () => {
    backend();
    renderPanel();
    expect(screen.getByText("140 questões")).toBeInTheDocument();
    expect(screen.getByText("Direito")).toBeInTheDocument();
    expect(screen.getByText("Erradas")).toBeInTheDocument();
  });

  it("mentions_questions_held_by_open_attempts", () => {
    backend();
    renderPanel({ summary: { ...summary, inOpenAttempt: 3 } });
    expect(screen.getByText("3 em tentativa aberta ficam de fora.")).toBeInTheDocument();
  });

  it("offers_only_the_allowed_orders", () => {
    backend();
    renderPanel();
    const options = screen.getAllByRole("option").map((option) => option.textContent);
    expect(options).toEqual(["Não vistas primeiro", "Aleatória"]);
  });

  it("starts_a_filter_session_with_count_and_order", async () => {
    const started = backend();
    renderPanel();
    await userEvent.click(await screen.findByRole("button", { name: "Resolver estas" }));
    await waitFor(() => expect(started).toHaveLength(1));
    expect(started[0]).toEqual({
      source: { kind: "filter", filter: emptyFilter, count: 20, order: "random" },
      feedbackMode: "study",
    });
    await waitFor(() => expect(window.location.hash).toBe("#/resolver?tentativa=att-1"));
  });

  it("starts_the_whole_caderno_as_a_collection_attempt", async () => {
    const started = backend();
    renderPanel({ filter: { ...emptyFilter, collectionId: "c-1" } });
    await userEvent.click(await screen.findByRole("button", { name: "Resolver estas" }));
    await waitFor(() => expect(started).toHaveLength(1));
    expect(started[0]).toEqual({
      source: { kind: "collection", collectionId: "c-1" },
      feedbackMode: "study",
    });
  });

  it("continues_the_open_attempt_of_a_whole_caderno", async () => {
    const started = backend({ openAttempt: "att-open" });
    renderPanel({ filter: { ...emptyFilter, collectionId: "c-1" } });
    await userEvent.click(await screen.findByRole("button", { name: "Continuar" }));
    expect(window.location.hash).toBe("#/resolver?tentativa=att-open");
    expect(started).toEqual([]);
  });

  it("disables_start_when_nothing_is_available", () => {
    backend();
    renderPanel({ summary: { ...summary, matching: 3, inOpenAttempt: 3 } });
    expect(screen.getByRole("button", { name: "Resolver estas" })).toBeDisabled();
    expect(screen.getByText("Todas estas questões estão em uma tentativa aberta.")).toBeVisible();
  });

  it("shows_a_start_error_inline", async () => {
    backend({ startError: "Falhou ao começar." });
    renderPanel();
    await userEvent.click(await screen.findByRole("button", { name: "Resolver estas" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Falhou ao começar.");
  });
});
