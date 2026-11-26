import { Show, createResource, createSignal } from "solid-js";
import { api } from "../../api";
import type { AttemptId } from "../../bindings/AttemptId";
import type { CollectionId } from "../../bindings/CollectionId";
import Modal from "../../components/Modal";
import CollectionPicker from "./CollectionPicker";

/** The caderno filter, and the attempt filter when Banco was opened from Histórico. */
export default function SourceFilter(props: {
  collectionId: CollectionId | null;
  attemptId: AttemptId | null;
  onCollection: (id: CollectionId | null) => void;
  onClearAttempt: () => void;
}) {
  const [picking, setPicking] = createSignal(false);
  const [collection] = createResource(
    () => props.collectionId,
    (id) => api.collection(id),
  );
  const [attempt] = createResource(
    () => props.attemptId,
    (id) => api.attempt(id),
  );
  const collectionTitle = () => (collection.error ? "Caderno" : (collection()?.title ?? "…"));
  const attemptTitle = () => (attempt.error ? "uma tentativa" : (attempt()?.title ?? "…"));

  return (
    <fieldset class="filter-group">
      <legend class="label">Caderno</legend>
      <Show
        when={props.collectionId}
        fallback={
          <span>
            <button type="button" class="btn" onClick={() => setPicking(true)}>
              Escolher caderno
            </button>
          </span>
        }
      >
        <Chip
          text={collectionTitle()}
          removeLabel="Remover caderno"
          onRemove={() => props.onCollection(null)}
        />
      </Show>
      <Show when={props.attemptId}>
        <Chip
          text={`Questões de: ${attemptTitle()}`}
          removeLabel="Remover tentativa"
          onRemove={props.onClearAttempt}
        />
      </Show>
      <Show when={picking()}>
        <Modal title="Escolher caderno" onClose={() => setPicking(false)}>
          <CollectionPicker
            onPick={(picked) => {
              setPicking(false);
              props.onCollection(picked.id);
            }}
          />
        </Modal>
      </Show>
    </fieldset>
  );
}

function Chip(props: { text: string; removeLabel: string; onRemove: () => void }) {
  return (
    <span>
      <span class="filter-chip">
        <span class="filter-chip-text">{props.text}</span>
        <button
          type="button"
          class="filter-chip-remove"
          aria-label={props.removeLabel}
          onClick={() => props.onRemove()}
        >
          ✕
        </button>
      </span>
    </span>
  );
}
