import { For, Show, createResource, createSignal } from "solid-js";
import { api } from "../../api";
import Modal from "../../components/Modal";
import { EmptyState, ResourceView } from "../../components/states";
import StartSessionModal from "../solve/StartSessionModal";
import { openAttempt } from "../../navigation";
import { formatDate, questionCount } from "../../format";

import { modeLabel } from "../../labels";

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
  const current = () => (detail.error ? undefined : detail());
  const active = () => current()?.attempts.find((a) => a.finishedAt === null);
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
                class="btn btn-primary"
                disabled={!current()?.questions}
                onClick={() => setStarting(true)}
              >
                Iniciar
              </button>
            }
          >
            {(attempt) => (
              <>
                <span class="muted">Há uma tentativa em andamento.</span>
                <button type="button" class="btn btn-primary" onClick={() => open(attempt().id)}>
                  Continuar
                </button>
              </>
            )}
          </Show>
        }
      >
        <ResourceView
          value={detail()}
          error={() => detail.error}
          pending={detail.loading}
          retrying={detail.loading}
          onRetry={() => refetch()}
        >
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
                          class="btn btn-link is-inline"
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
        </ResourceView>
      </Modal>
    </Show>
  );
}
