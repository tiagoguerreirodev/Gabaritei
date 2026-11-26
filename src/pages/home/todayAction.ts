import type { ActiveAttempt } from "../../bindings/ActiveAttempt";
import type { HomeSummary } from "../../bindings/HomeSummary";
import type { WeakTopic } from "../../bindings/WeakTopic";

/** The one thing the home page puts first. */
export type TodayAction =
  | { kind: "resume"; attempt: ActiveAttempt }
  | { kind: "reviews"; due: number }
  | { kind: "weakest"; topic: WeakTopic }
  | { kind: "start" };

/**
 * Most urgent first: resume the newest open attempt, then the due reviews, then the weakest
 * topic. With none of these, start from the Banco.
 */
export function todayAction(summary: HomeSummary): TodayAction {
  const [newest] = summary.activeAttempts;
  if (newest) return { kind: "resume", attempt: newest };
  if (summary.dueReviews > 0) return { kind: "reviews", due: summary.dueReviews };
  if (summary.weakestTopic) return { kind: "weakest", topic: summary.weakestTopic };
  return { kind: "start" };
}
