import { Show } from "solid-js";
import Modal from "../../components/Modal";

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
        <Modal
          title="Finalizar tentativa?"
          onClose={() => props.onClose()}
          footer={
            <>
              <button type="button" onClick={() => props.onClose()}>
                Continuar resolvendo
              </button>
              <button type="button" class="primary" onClick={() => props.onFinish()}>
                Finalizar
              </button>
            </>
          }
        >
          <p data-testid="finish-summary">
            {props.answered} respondidas e {props.total - props.answered} em branco.
          </p>
          <p class="muted">Depois de finalizar, as respostas não podem mais mudar.</p>
        </Modal>
      </Show>
      <Show when={props.dialog === "abandon"}>
        <Modal
          title="Abandonar tentativa?"
          onClose={() => props.onClose()}
          footer={
            <>
              <button type="button" onClick={() => props.onClose()}>
                Voltar
              </button>
              <button type="button" class="primary danger" onClick={() => props.onAbandon()}>
                Abandonar
              </button>
            </>
          }
        >
          <p>A tentativa e suas respostas serão apagadas e não entram no histórico.</p>
        </Modal>
      </Show>
    </>
  );
}
