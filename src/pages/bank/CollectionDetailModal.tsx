import { For, Match, Show, Switch, createResource, createSignal } from "solid-js";
import { api } from "../../api";
import Modal from "../../components/Modal";
import { EmptyState, ErrorState, Loading } from "../../components/states";
import StartSessionModal from "../solve/StartSessionModal";
import { openAttempt } from "../../navigation";
import { formatDate, questionCount } from "../../format";

const modeLabel = { study: "Estudo", exam: "Simulado" } as const;

/** A collection's details and its attempts, with buttons to start or continue a session. */
export default function CollectionDetailModal(props: {
  id: string;
  title: string;
  onClose: () => void;
}) {
  const [detail, { refetch }] = createResource(
    () => props.id,
    (id) => api.collection(id),
  );

  const [starting, setStarting] = createSignal(false);
  const active = () => detail()?.attempts.find((a) => a.finishedAt === null);
  const open = (id: string) => {
    props.onClose();
    openAttempt(id);
  };

  return (
    <Show
      when={!starting()}
      fallback={
        <StartSessionModal
          title={props.title}
          source={{ kind: "collection", collectionId: props.id }}
          onClose={() => setStarting(false)}
          onStarted={() => props.onClose()}
        />
      }
    >
      <Modal
        title={props.title}
        onClose={() => props.onClose()}
        footer={
          <Show
            when={active()}
            fallback={
              <button
                type="button"
                class="primary"
                disabled={!detail() || detail()?.questions === 0}
                onClick={() => setStarting(true)}
              >
                Iniciar
              </button>
            }
          >
            {(attempt) => (
              <>
                <span class="muted">Há uma tentativa em andamento.</span>
                <button type="button" class="primary" onClick={() => open(attempt().id)}>
                  Continuar
                </button>
              </>
            )}
          </Show>
        }
      >
        <Switch>
          <Match when={detail.error as unknown}>
            {(error) => (
              <ErrorState error={error()} onRetry={() => refetch()} busy={detail.loading} />
            )}
          </Match>
          <Match when={detail.loading}>
            <Loading />
          </Match>
          <Match when={detail()}>
            {(loaded) => (
              <>
                <dl class="facts">
                  <dt>Questões</dt>
                  <dd data-testid="collection-questions">{questionCount(loaded().questions)}</dd>
                  <Show when={loaded().contest}>
                    <dt>Concurso</dt>
                    <dd>{loaded().contest}</dd>
                  </Show>
                  <Show when={loaded().board}>
                    <dt>Banca</dt>
                    <dd>{loaded().board}</dd>
                  </Show>
                  <dt>Criado em</dt>
                  <dd>{formatDate(loaded().createdAt)}</dd>
                </dl>
                <h3>Tentativas</h3>
                <Show
                  when={loaded().attempts.length > 0}
                  fallback={<EmptyState title="Nenhuma tentativa ainda." />}
                >
                  <ul class="attempts">
                    <For each={loaded().attempts}>
                      {(attempt) => (
                        <li>
                          <button
                            type="button"
                            class="link-button"
                            onClick={() => open(attempt.id)}
                          >
                            {attempt.finishedAt ? "Ver resultado" : "Continuar"}
                          </button>{" "}
                          {formatDate(attempt.startedAt)} · {modeLabel[attempt.feedbackMode]} ·{" "}
                          {attempt.finishedAt
                            ? `concluída, ${attempt.answered} de ${attempt.total} respondidas`
                            : `em andamento, ${attempt.answered} de ${attempt.total} respondidas`}
                        </li>
                      )}
                    </For>
                  </ul>
                </Show>
              </>
            )}
          </Match>
        </Switch>
      </Modal>
    </Show>
  );
}
