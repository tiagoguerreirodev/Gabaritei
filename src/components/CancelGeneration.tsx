import { Show, createSignal } from "solid-js";
import { api } from "../api";
import { cancelDiscardsDraft } from "../generation";
import ConfirmModal from "./ConfirmModal";
import { showError } from "./toasts";

/**
 * Cancelar for the running job. It shows a pending state until the run ends, and asks first
 * when cancelling would throw the generated draft away (during the blind check).
 */
export default function CancelGeneration() {
  const [pending, setPending] = createSignal(false);
  const [confirming, setConfirming] = createSignal(false);

  const cancel = async () => {
    setPending(true);
    try {
      // The button goes away with the running state; a `false` means nothing was running.
      if (!(await api.generationCancel())) setPending(false);
    } catch (error) {
      setPending(false);
      showError(error);
    }
  };

  return (
    <>
      <button
        type="button"
        class="secondary"
        disabled={pending()}
        onClick={() => (cancelDiscardsDraft() ? setConfirming(true) : void cancel())}
      >
        {pending() ? "Cancelando…" : "Cancelar"}
      </button>
      <Show when={confirming()}>
        <ConfirmModal
          title="Cancelar a geração?"
          confirmLabel="Cancelar geração"
          cancelLabel="Continuar gerando"
          danger
          onClose={() => setConfirming(false)}
          onConfirm={() => {
            setConfirming(false);
            void cancel();
          }}
        >
          <p>
            As questões já foram geradas, mas a conferência ainda não terminou. Cancelar agora
            descarta o pacote e nada é salvo.
          </p>
        </ConfirmModal>
      </Show>
    </>
  );
}
