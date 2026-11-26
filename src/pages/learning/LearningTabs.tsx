import { A } from "@solidjs/router";
import { For, Show, createSignal } from "solid-js";
import type { LearningItem } from "../../bindings/LearningItem";
import type { LearningOverview } from "../../bindings/LearningOverview";
import type { SessionSource } from "../../bindings/SessionSource";
import { EmptyState } from "../../components/states";
import { formatNumber } from "../../format";
import LearningRow from "./LearningRow";

type TabId = "today" | "overdue" | "upcoming" | "errors";

const TABS: { id: TabId; label: string; empty: string; matches: (i: LearningItem) => boolean }[] = [
  {
    id: "today",
    label: "Para hoje",
    empty: "Nenhuma revisão para hoje.",
    matches: (i) => i.review?.state === "today",
  },
  {
    id: "overdue",
    label: "Atrasadas",
    empty: "Nenhuma revisão atrasada.",
    matches: (i) => i.review?.state === "overdue",
  },
  {
    id: "upcoming",
    label: "Próximas",
    empty: "Nenhuma revisão agendada.",
    matches: (i) => i.review?.state === "upcoming",
  },
  {
    id: "errors",
    label: "Caderno de erros",
    empty: "Nenhum erro por enquanto.",
    matches: (i) => i.isError,
  },
];

type PracticeRequest = { title: string; source: SessionSource };

/** The reviews and errors, one tab each, with the practice actions of the page. */
export default function LearningTabs(props: {
  data: LearningOverview;
  includeBlanks: boolean;
  onIncludeBlanks: (value: boolean) => void;
  onPractice: (request: PracticeRequest) => void;
}) {
  const itemsOf = (tab: (typeof TABS)[number]) => props.data.items.filter(tab.matches);
  const firstTabWithItems = () => TABS.find((t) => itemsOf(t).length > 0)?.id ?? "today";
  const [selected, setSelected] = createSignal<TabId>(firstTabWithItems());
  const tab = () => TABS.find((t) => t.id === selected()) ?? TABS[0]!;

  return (
    <>
      <div class="learning-actions">
        <button
          type="button"
          class="primary"
          disabled={props.data.practice === 0}
          onClick={() =>
            props.onPractice({ title: "Praticar revisões", source: { kind: "reviews" } })
          }
        >
          Revisar tudo de hoje ({formatNumber(props.data.practice)})
        </button>
        <Show when={props.data.dueInOpenAttempt > 0}>
          <span class="muted">{formatNumber(props.data.dueInOpenAttempt)} em tentativa aberta</span>
        </Show>
      </div>

      <div class="tabs" role="tablist" aria-label="Revisões e erros">
        <For each={TABS}>
          {(t) => (
            <button
              type="button"
              role="tab"
              id={`learning-tab-${t.id}`}
              aria-selected={selected() === t.id}
              aria-controls="learning-panel"
              class="tab"
              onClick={() => setSelected(t.id)}
            >
              {t.label} <span class="badge">{formatNumber(itemsOf(t).length)}</span>
            </button>
          )}
        </For>
      </div>

      <div role="tabpanel" id="learning-panel" aria-labelledby={`learning-tab-${selected()}`}>
        <Show when={selected() === "errors"}>
          <div class="learning-actions">
            <button
              type="button"
              class="secondary"
              disabled={props.data.practiceErrors === 0}
              onClick={() =>
                props.onPractice({
                  title: "Praticar erros",
                  source: { kind: "errors", includeBlanks: props.includeBlanks },
                })
              }
            >
              Praticar erros ({formatNumber(props.data.practiceErrors)})
            </button>
            <label class="learning-blanks">
              <input
                type="checkbox"
                checked={props.includeBlanks}
                onChange={(event) => props.onIncludeBlanks(event.currentTarget.checked)}
              />
              Incluir questões em branco
            </label>
          </div>
        </Show>
        <Show
          when={itemsOf(tab()).length > 0}
          fallback={
            <EmptyState
              title={tab().empty}
              action={
                props.data.items.length === 0 ? (
                  <A class="button-link primary" href="/banco">
                    Ir para o Banco
                  </A>
                ) : undefined
              }
            />
          }
        >
          <h2 class="learning-group">
            <span>{tab().label}</span>
            <span class="muted">{formatNumber(itemsOf(tab()).length)}</span>
          </h2>
          <ul class="learning-list" aria-label="Questões">
            <For each={itemsOf(tab())}>{(item) => <LearningRow item={item} />}</For>
          </ul>
        </Show>
      </div>
    </>
  );
}
