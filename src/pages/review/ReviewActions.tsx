import { A, useNavigate } from "@solidjs/router";
import { Show, createSignal } from "solid-js";
import { createAction } from "../../actions";
import { api } from "../../api";
import type { DraftSummary } from "../../bindings/DraftSummary";
import type { ImportResult } from "../../bindings/ImportResult";
import ConfirmModal from "../../components/ConfirmModal";
import { showSuccess, showToast } from "../../components/toasts";
import { formatNumber } from "../../format";
import { refreshWaitingDrafts } from "../../generation";

export default function ReviewActions(props: { summary: DraftSummary; onChanged: () => void }) {
  const navigate = useNavigate();
  const { active: busy, run } = createAction();
  const [requestingCheck, setRequestingCheck] = createSignal(false);
  const [confirmingDiscard, setConfirmingDiscard] = createSignal(false);
  const s = () => props.summary;
  const waiting = () => s().status === "review";
  const checking = () => s().checking;
  const confirmed = () => s().confirmed;

  const imported = async (result: ImportResult) => {
    await refreshWaitingDrafts();
    showSuccess(
      `${formatNumber(result.imported)} questões importadas para “${result.title}”` +
        (result.skipped > 0 ? `; ${formatNumber(result.skipped)} ficaram de fora.` : "."),
    );
    navigate("/banco");
  };

  const importAll = () => {
    const { id, needsCheck } = s();
    const onChanged = props.onChanged;
    return run(async () => {
      setRequestingCheck(needsCheck);
      const outcome = await api.draftImportAll(id).finally(() => setRequestingCheck(false));
      if (outcome.kind === "pending") {
        onChanged();
        showToast(outcome.message);
      } else {
        await imported(outcome.result);
      }
    });
  };

  const importConfirmed = () => {
    const id = s().id;
    return run(async () => imported(await api.draftImport(id, "confirmed")));
  };

  const discard = () => {
    const id = s().id;
    return run(async () => {
      await api.draftDiscard(id);
      await refreshWaitingDrafts();
      showToast("Pacote descartado.");
      navigate("/gerar");
    });
  };

  const rerunCheck = () => {
    const id = s().id;
    return run(async () => {
      await api.blindCheckStart(id);
      showToast("Conferindo de novo. Acompanhe pelo aviso no topo.");
    });
  };

  return (
    <Show
      when={waiting()}
      fallback={
        <p class="muted">
          Este pacote já foi {s().status === "imported" ? "importado" : "descartado"}.{" "}
          <A href="/gerar">Gerar outro</A>.
        </p>
      }
    >
      <Show when={requestingCheck() && !checking()}>
        <p role="status">Solicitando conferência e importação…</p>
      </Show>
      <div class="review-actions">
        <button
          type="button"
          class="btn btn-primary"
          disabled={busy() || checking() || s().problems.length > 0}
          onClick={() => void importAll()}
        >
          {s().needsCheck ? "Conferir e importar tudo" : "Importar tudo"}
        </button>
        <button
          type="button"
          class="btn"
          disabled={busy() || checking() || confirmed() === 0}
          onClick={() => void importConfirmed()}
        >
          Importar só as confirmadas
        </button>
        <Show when={!checking() && s().needsCheck}>
          <button type="button" class="btn" disabled={busy()} onClick={() => void rerunCheck()}>
            Conferir de novo
          </button>
        </Show>
        <button
          type="button"
          class="btn"
          disabled={busy() || checking()}
          onClick={() => setConfirmingDiscard(true)}
        >
          Descartar
        </button>
      </div>
      <Show when={confirmingDiscard()}>
        <ConfirmModal
          title="Descartar o pacote?"
          confirmLabel="Descartar pacote"
          cancelLabel="Voltar"
          danger
          busy={busy()}
          onClose={() => setConfirmingDiscard(false)}
          onConfirm={() => {
            setConfirmingDiscard(false);
            void discard();
          }}
        >
          <p>
            As {formatNumber(s().count)} questões deste pacote serão apagadas e nada entra no banco.
            <Show when={confirmed() > 0}>
              {" "}
              Para aproveitar parte delas, use “Importar só as confirmadas”.
            </Show>
          </p>
        </ConfirmModal>
      </Show>
      <Show when={s().problems.length > 0}>
        <p class="muted">
          “Importar tudo” fica bloqueado enquanto houver problemas. As questões sem problemas e
          confirmadas ainda podem ser importadas.
        </p>
      </Show>
    </Show>
  );
}
