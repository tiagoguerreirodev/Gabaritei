import type { FeedbackMode } from "../../bindings/FeedbackMode";
import type { QuestionFilter } from "../../bindings/QuestionFilter";
import type { SessionOrder } from "../../bindings/SessionOrder";
import type { StudyStatus } from "../../bindings/StudyStatus";

/** Everything Banco keeps in the URL. */
export interface BancoState {
  filter: QuestionFilter;
  /** `null` means the backend's default: the first allowed order. */
  order: SessionOrder | null;
  mode: FeedbackMode;
}

export type BancoParams = Record<string, string | undefined>;

export const emptyFilter: QuestionFilter = {
  topicIds: [],
  statuses: [],
  collectionId: null,
  attemptId: null,
};

export const emptyBancoState: BancoState = { filter: emptyFilter, order: null, mode: "study" };

export const studyStatuses: StudyStatus[] = ["unseen", "wrong", "blank", "due", "flagged"];
export const sessionOrders: SessionOrder[] = [
  "unseenFirst",
  "random",
  "dueFirst",
  "recentErrorsFirst",
];
const feedbackModes: FeedbackMode[] = ["study", "exam"];

/** Reads Banco's state from the URL query. Unknown or stale values are dropped. */
export function parseBanco(params: BancoParams): BancoState {
  return {
    filter: {
      topicIds: splitList(params.assuntos).flatMap(parseTopicId),
      statuses: splitList(params.status).filter((value) => isOneOf(studyStatuses, value)),
      collectionId: params.caderno || null,
      attemptId: params.tentativa || null,
    },
    order: params.ordem && isOneOf(sessionOrders, params.ordem) ? params.ordem : null,
    mode: params.modo && isOneOf(feedbackModes, params.modo) ? params.modo : "study",
  };
}

/** The URL query for a state; empty parts are `undefined` so the router leaves them out. */
export function bancoSearch(state: BancoState): BancoParams {
  const { filter } = state;
  return {
    assuntos: joinList(filter.topicIds),
    status: joinList(filter.statuses),
    caderno: filter.collectionId ?? undefined,
    tentativa: filter.attemptId ?? undefined,
    ordem: state.order ?? undefined,
    modo: state.mode === "study" ? undefined : state.mode,
  };
}

/** A link to Banco with the given parts filled in, for presets and shortcuts. */
export function bancoHref(state: Partial<BancoState>): string {
  const params = bancoSearch({ ...emptyBancoState, ...state });
  const query = Object.entries(params)
    .filter((entry): entry is [string, string] => entry[1] !== undefined)
    .map(([key, value]) => `${key}=${encodeURIComponent(value).replace(/%2C/g, ",")}`)
    .join("&");
  return query ? `#/banco?${query}` : "#/banco";
}

function splitList(value: string | undefined): string[] {
  return value ? value.split(",").filter((part) => part !== "") : [];
}

function joinList(values: (string | number)[]): string | undefined {
  return values.length > 0 ? values.join(",") : undefined;
}

function parseTopicId(text: string): number[] {
  const id = Number(text);
  return Number.isInteger(id) && id > 0 ? [id] : [];
}

function isOneOf<T extends string>(allowed: readonly T[], value: string): value is T {
  return (allowed as readonly string[]).includes(value);
}
