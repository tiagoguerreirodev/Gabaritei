import { Show, createSignal } from "solid-js";
import { api } from "../../api";
import Modal from "../../components/Modal";
import { showError, showToast } from "../../components/toasts";
import type { Busy } from "./busy";

/** Moves the study data between computers with one file. */
export default function DataFileSection(props: {
  /** Shared with the backup section, so only one action runs at a time. */
  busy: Busy;
  /** Called after an import replaced the data, so the app shows the imported state. */
  onImported?: () => void;
}) {
  // Read once: these are plain objects and functions, not something that changes.
  // eslint-disable-next-line solid/reactivity
  const { busy, onImported } = props;
  const [confirming, setConfirming] = createSignal(false);

  const exportData = () =>
    busy.run(async () => {
      try {
        if (await api.exportData()) showToast("Arquivo de dados salvo.");
      } catch (error) {
        showError(error);
      }
    });

  const importData = () =>
    busy.run(async () => {
      try {
        const imported = await api.importData();
        setConfirming(false);
        if (imported) {
          showToast("Dados importados.");
          if (onImported) onImported();
          else window.location.reload();
        }
      } catch (error) {
        setConfirming(false);
        showError(error);
      }
    });

  return (
    <section class="card" aria-labelledby="data-file-title">
      <h2 id="data-file-title">Levar os dados para outro computador</h2>
      <p class="muted">
        Exporte um arquivo com todas as suas questões, tentativas e imagens e importe-o no outro
        computador. Não há sincronização: <strong>use um computador de cada vez</strong>. Quem
        importa fica exatamente com o que está no arquivo, e o que havia antes é substituído.
      </p>
      <p class="actions">
        <button
          type="button"
          class="primary"
          disabled={busy.active()}
          onClick={() => void exportData()}
        >
          Exportar dados…
        </button>
        <button type="button" disabled={busy.active()} onClick={() => setConfirming(true)}>
          Importar dados…
        </button>
      </p>
      <Show when={confirming()}>
        <Modal
          title="Importar dados?"
          onClose={() => !busy.active() && setConfirming(false)}
          footer={
            <>
              <button type="button" disabled={busy.active()} onClick={() => setConfirming(false)}>
                Cancelar
              </button>
              <button
                type="button"
                class="primary danger"
                disabled={busy.active()}
                onClick={() => void importData()}
              >
                {busy.active() ? "Importando…" : "Escolher arquivo e importar"}
              </button>
            </>
          }
        >
          <p>
            Todos os dados deste computador serão substituídos pelos do arquivo. Não há mistura dos
            dois: o que só existe aqui será perdido.
          </p>
          <p class="muted">
            Antes de substituir, o Gabaritei guarda uma cópia dos dados atuais na pasta de backups,
            e ela aparece na lista para você voltar atrás. Um arquivo inválido não altera nada.
          </p>
        </Modal>
      </Show>
    </section>
  );
}
