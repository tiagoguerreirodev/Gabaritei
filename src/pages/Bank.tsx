import { For, Match, Switch, createSignal } from "solid-js";
import CollectionsView from "./bank/CollectionsView";
import TopicTreeView from "./bank/TopicTreeView";

type Tab = "topics" | "collections";

const tabs: { id: Tab; label: string }[] = [
  { id: "topics", label: "Assuntos" },
  { id: "collections", label: "Cadernos" },
];

/** Browse the bank by topic tree and by collection (spec §3.1). */
export default function Bank() {
  const [tab, setTab] = createSignal<Tab>("topics");

  return (
    <section class="page">
      <h1>Banco</h1>
      <div class="tabs" role="tablist" aria-label="Visualização do banco">
        <For each={tabs}>
          {(t) => (
            <button
              type="button"
              role="tab"
              id={`tab-${t.id}`}
              aria-selected={tab() === t.id}
              aria-controls={`panel-${t.id}`}
              class="tab"
              onClick={() => setTab(t.id)}
            >
              {t.label}
            </button>
          )}
        </For>
      </div>
      <div role="tabpanel" id={`panel-${tab()}`} aria-labelledby={`tab-${tab()}`}>
        <Switch>
          <Match when={tab() === "topics"}>
            <TopicTreeView />
          </Match>
          <Match when={tab() === "collections"}>
            <CollectionsView />
          </Match>
        </Switch>
      </div>
    </section>
  );
}
