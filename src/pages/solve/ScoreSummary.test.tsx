import { render, screen } from "@solidjs/testing-library";
import { mockIPC } from "@tauri-apps/api/mocks";
import { describe, expect, it } from "vitest";
import type { Score } from "../../bindings/Score";
import ScoreSummary from "./ScoreSummary";

const score: Score = {
  correct: 1,
  wrong: 2,
  blank: 0,
  annulled: 0,
  evaluated: 3,
  achievement: 1 / 3,
  accuracy: 1 / 3,
};

describe("ScoreSummary", () => {
  it("links to the wrong answers of this attempt, even when the extras fail to load", () => {
    mockIPC(() => {
      throw new Error("sem fechamento");
    });
    render(() => <ScoreSummary attemptId="att-1" score={score} />);
    expect(screen.getByRole("link", { name: "Refazer só os erros" })).toHaveAttribute(
      "href",
      "#/banco?status=wrong&tentativa=att-1",
    );
    expect(screen.getByRole("link", { name: "Voltar ao Início" })).toBeVisible();
    expect(screen.queryByTestId("recap-comparison")).toBeNull();
  });
});
