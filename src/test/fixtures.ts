import type { GroupStats } from "../bindings/GroupStats";
import type { HomeSummary } from "../bindings/HomeSummary";
import type { Performance } from "../bindings/Performance";

/** Group stats from raw counts, deriving the ratios the way the backend does. */
export function stats(
  correct: number,
  wrong: number,
  blank: number,
  available: number,
): GroupStats {
  const evaluated = correct + wrong + blank;
  const answered = correct + wrong;
  return {
    score: {
      correct,
      wrong,
      blank,
      annulled: 0,
      evaluated,
      achievement: evaluated ? correct / evaluated : null,
      accuracy: answered ? correct / answered : null,
    },
    answered,
    available,
    coverage: available ? answered / available : null,
    smallSample: answered < 5,
  };
}

export const emptyPerformance: Performance = {
  overall: stats(0, 0, 0, 0),
  areas: [],
  weakest: [],
  trend: [],
  filters: { contests: [], boards: [] },
};

export const emptyHome: HomeSummary = {
  questions: 0,
  overall: stats(0, 0, 0, 0),
  areas: [],
  activeAttempts: [],
  dueReviews: 0,
  draftsWaiting: 0,
};
