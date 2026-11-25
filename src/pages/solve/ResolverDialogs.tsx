import { Show } from "solid-js";
import ConfirmModal from "../../components/ConfirmModal";

export type ResolverDialog = "finish" | "abandon" | null;

export default function ResolverDialogs(props: {
  dialog: ResolverDialog;
  answered: number;
  total: number;
  onClose: () => void;
  onFinish: () => void;
  onAbandon: () => void;
}) {
  return (
    <>
      <Show when={props.dialog === "finish"}>
        <ConfirmModal
          title="Finalizar tentativa?"
          cancelLabel="Continuar resolvendo"
          confirmLabel="Finalizar"
          onClose={() => props.onClose()}
          onConfirm={() => props.onFinish()}
        >
          <p data-testid="finish-summary">
            {props.answered} respondidas e {props.total - props.answered} em branco.
          </p>
          <p class="muted">Depois de finalizar, as respostas não podem mais mudar.</p>
        </ConfirmModal>
      </Show>
      <Show when={props.dialog === "abandon"}>
        <ConfirmModal
          title="Abandonar tentativa?"
          cancelLabel="Voltar"
          confirmLabel="Abandonar"
          danger
          onClose={() => props.onClose()}
          onConfirm={() => props.onAbandon()}
        >
          <p>A tentativa e suas respostas serão apagadas e não entram no histórico.</p>
        </ConfirmModal>
      </Show>
    </>
  );
}
