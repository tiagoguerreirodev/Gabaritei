import { render, screen, waitFor } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { describe, expect, it, vi } from "vitest";
import SourceFilter from "./SourceFilter";
import StatusFilter from "./StatusFilter";
import TopicFilter from "./TopicFilter";
import { summary, tree } from "./bankFixtures";

describe("TopicFilter", () => {
  const renderTree = (selected: number[]) => {
    const onChange = vi.fn();
    render(() => (
      <TopicFilter tree={tree} counts={summary.byTopic} selected={selected} onChange={onChange} />
    ));
    return onChange;
  };

  it("checking_an_area_selects_all_its_topics", async () => {
    const onChange = renderTree([]);
    await userEvent.click(screen.getByRole("checkbox", { name: /^Direito\s*140$/ }));
    expect(onChange).toHaveBeenCalledWith([1, 2]);
  });

  it("a_partly_checked_matter_is_indeterminate", () => {
    renderTree([1]);
    const matter = screen.getByRole<HTMLInputElement>("checkbox", { name: /Constitucional/ });
    expect(matter.indeterminate).toBe(true);
    expect(matter.checked).toBe(false);
  });

  it("unchecking_a_matter_clears_its_topics", async () => {
    const onChange = renderTree([1, 2]);
    await userEvent.click(screen.getByRole("checkbox", { name: /Constitucional/ }));
    expect(onChange).toHaveBeenCalledWith([]);
  });

  it("shows_the_count_of_each_topic", () => {
    renderTree([]);
    expect(screen.getByRole("checkbox", { name: /^Controle\s*100$/ })).toBeInTheDocument();
  });
});

describe("StatusFilter", () => {
  it("shows_each_status_with_its_count", () => {
    render(() => (
      <StatusFilter counts={summary.byStatus} selected={["wrong"]} onChange={vi.fn()} />
    ));
    expect(screen.getByRole("checkbox", { name: /^Nunca resolvidas\s*61$/ })).not.toBeChecked();
    expect(screen.getByRole("checkbox", { name: /^Erradas\s*22$/ })).toBeChecked();
    expect(screen.getByRole("checkbox", { name: /^Sinalizadas\s*3$/ })).toBeInTheDocument();
  });

  it("reports_the_statuses_in_enum_order", async () => {
    const onChange = vi.fn();
    render(() => <StatusFilter counts={summary.byStatus} selected={["due"]} onChange={onChange} />);
    await userEvent.click(screen.getByRole("checkbox", { name: /^Erradas/ }));
    expect(onChange).toHaveBeenCalledWith(["wrong", "due"]);
  });
});

describe("SourceFilter", () => {
  const collection = (id: string) => ({
    id,
    title: id === "c-1" ? "TRF5 2024" : "Outro",
    kind: "imported",
    contest: null,
    board: null,
    questions: 30,
    createdAt: "2026-11-11T00:00:00.000Z",
    lastFinishedAt: null,
    lastAchievement: null,
    activeAttemptId: null,
  });
  const backend = () =>
    mockIPC((command, args) => {
      if (command === "collections")
        return {
          items: [collection("c-1"), collection("c-2")],
          total: 2,
          page: 0,
          pageSize: 20,
          contests: [],
          boards: [],
        };
      if (command === "collection") return { ...collection("c-1"), attempts: [] };
      if (command === "attempt")
        return { id: (args as { id: string }).id, title: "Revisões de hoje" };
    });

  it("picking_a_caderno_reports_its_id", async () => {
    backend();
    const onCollection = vi.fn();
    render(() => (
      <SourceFilter
        collectionId={null}
        attemptId={null}
        onCollection={onCollection}
        onClearAttempt={vi.fn()}
      />
    ));
    await userEvent.click(screen.getByRole("button", { name: "Escolher caderno" }));
    await userEvent.click(await screen.findByRole("button", { name: /TRF5 2024/ }));
    expect(onCollection).toHaveBeenCalledWith("c-1");
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("shows_the_chosen_caderno_as_a_chip_that_can_be_removed", async () => {
    backend();
    const onCollection = vi.fn();
    render(() => (
      <SourceFilter
        collectionId="c-1"
        attemptId={null}
        onCollection={onCollection}
        onClearAttempt={vi.fn()}
      />
    ));
    expect(await screen.findByText("TRF5 2024")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Remover caderno" }));
    expect(onCollection).toHaveBeenCalledWith(null);
  });

  it("removing_the_attempt_chip_clears_it", async () => {
    backend();
    const onClearAttempt = vi.fn();
    render(() => (
      <SourceFilter
        collectionId={null}
        attemptId="a-1"
        onCollection={vi.fn()}
        onClearAttempt={onClearAttempt}
      />
    ));
    expect(await screen.findByText("Questões de: Revisões de hoje")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Remover tentativa" }));
    expect(onClearAttempt).toHaveBeenCalled();
  });
});
