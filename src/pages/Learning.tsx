import { Show, createEffect, createResource, createSignal, onCleanup } from "solid-js";
import { api } from "../api";
import type { SessionSource } from "../bindings/SessionSource";
import { Loading, ResourceView } from "../components/states";
import LearningTabs from "./learning/LearningTabs";
import StartSessionModal from "./solve/StartSessionModal";

/**
 * Erros e revisões: what is due, what is coming and the error notebook, with a way to
 * practice. Rows show no answer keys; they link to the past attempt, where the question has
 * already been revealed.
 */
export default function Learning() {
  const [includeBlanks, setIncludeBlanks] = createSignal(false);
  // An object source: Solid doesn't fetch while a source is `false`.
  const [overview, { refetch }] = createResource(
    () => ({ blanks: includeBlanks() }),
    ({ blanks }) => api.learningOverview(blanks),
  );
  const [practice, setPractice] = createSignal<{ title: string; source: SessionSource } | null>(
    null,
  );
  createEffect(() => {
    if (overview.error || overview.loading) return;
    const at = overview()?.refreshAt;
    if (!at) return;
    const timer = setTimeout(() => void refetch(), Math.max(0, Date.parse(at) - Date.now()));
    onCleanup(() => clearTimeout(timer));
  });

  return (
    <section class="page">
      <h1>Erros e revisões</h1>

      <ResourceView
        value={overview()}
        error={() => overview.error}
        pending={overview.state === "pending"}
        retrying={overview.loading}
        onRetry={() => refetch()}
        placeholder={<Loading variant="skeleton" />}
      >
        {(data) => (
          <>
            <LearningTabs
              data={data()}
              includeBlanks={includeBlanks()}
              onIncludeBlanks={setIncludeBlanks}
              onPractice={setPractice}
            />
          </>
        )}
      </ResourceView>

      <Show when={practice()}>
        {(session) => (
          <StartSessionModal
            title={session().title}
            source={session().source}
            onClose={() => setPractice(null)}
          />
        )}
      </Show>
    </section>
  );
}
