import type { SelectionSummary } from "../../bindings/SelectionSummary";
import type { TopicTree } from "../../bindings/TopicTree";
import { stats } from "../../test/fixtures";

/** A small synthetic tree: Direito › Constitucional (topics 1 and 2), and an empty area. */
export const tree: TopicTree = {
  questions: 140,
  areas: [
    {
      name: "Direito",
      stats: stats(0, 0, 0, 140),
      matters: [
        {
          name: "Constitucional",
          stats: stats(0, 0, 0, 140),
          topics: [
            { topicId: 1, name: "Controle", stats: stats(0, 0, 0, 100), trend: [] },
            { topicId: 2, name: "Direitos fundamentais", stats: stats(0, 0, 0, 40), trend: [] },
          ],
        },
      ],
    },
  ],
};

export const summary: SelectionSummary = {
  matching: 140,
  inOpenAttempt: 0,
  byArea: [{ area: "Direito", questions: 140 }],
  byStatus: [
    { status: "unseen", questions: 61 },
    { status: "wrong", questions: 22 },
    { status: "blank", questions: 4 },
    { status: "due", questions: 9 },
    { status: "flagged", questions: 3 },
  ],
  byTopic: [
    { topicId: 1, questions: 100 },
    { topicId: 2, questions: 40 },
  ],
  allowedOrders: ["unseenFirst", "random"],
};
