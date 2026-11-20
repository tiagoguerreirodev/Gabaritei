import { For, Show, createResource, createSignal } from "solid-js";
import { api } from "../../api";
import type { BackupInfo } from "../../bindings/BackupInfo";
import type { BackupStatus } from "../../bindings/BackupStatus";
import Modal from "../../components/Modal";
import { ErrorState, Loading } from "../../components/states";
import { showError, showToast } from "../../components/toasts";
import type { Busy } from "./busy";
import { openFolder } from "./openFolder";

const dateTime = (iso: string) =>
  new Date(iso).toLocaleString("pt-BR", { dateStyle: "short", timeStyle: "short" });

const megabytes = (bytes: number) =>
  `${(bytes / (1024 * 1024)).toLocaleString("pt-BR", { maximumFractionDigits: 1 })} MB`;

/** Backups of the study data: the folder, a backup now, and restore. */
export default function BackupSection(props: {
  /** Shared with the data-file section, so only one action runs at a time. */
  busy: Busy;
  /** Called after a restore replaced the data, so the app shows the restored state. */
  onRestored?: () => void;
}) {
  // Read once: these are plain objects and functions, not something that changes.
  // eslint-disable-next-line solid/reactivity
  const { busy, onRestored } = props;
  const [status, { mutate, refetch }] = createResource(() => api.backupStatus());
  const [restoring, setRestoring] = createSignal<BackupInfo | null>(null);

  const run = (work: () => Promise<void>) =>
    busy.run(async () => {
      try {
        await work();
      } catch (error) {
        showError(error);
        // The failure may have been recorded (a backup that failed), so show it.
        void refetch();
      }
    });

  const backupNow = () =>
    run(async () => {
      await api.backupNow();
      showToast("Backup feito.");
      mutate(await api.backupStatus());
    });

  const chooseFolder = () =>
    run(async () => {
      const chosen = await api.backupChooseFolder();
      if (chosen) {
        mutate(chosen);
        showToast("Pasta dos backups salva.");
      }
    });

  const useDefaultFolder = () =>
    run(async () => {
      mutate(await api.backupUseDefaultFolder());
      showToast("Voltou para a pasta padrão.");
    });

  const restore = (backup: BackupInfo) =>
    run(async () => {
      await api.backupRestore(backup.name);
      setRestoring(null);
      showToast("Dados restaurados.");
      if (onRestored) onRestored();
      else window.location.reload();
    });

  return (
    <section class="card" aria-labelledby="backup-title">
      <h2 id="backup-title">Backups</h2>
      <p class="muted">
        O Gabaritei faz um backup por dia enquanto está aberto e guarda os 7 mais recentes. Os
        backups feitos por você ficam à parte e não tomam o lugar dos diários.
      </p>
      <Show when={!status.error} fallback={<ErrorState error={status.error as unknown} />}>
        <Show when={status()} fallback={<Loading />}>
          {(current) => (
            <Content
              status={current()}
              busy={busy.active()}
              onBackupNow={() => void backupNow()}
              onChooseFolder={() => void chooseFolder()}
              onUseDefaultFolder={() => void useDefaultFolder()}
              onRestore={setRestoring}
            />
          )}
        </Show>
      </Show>
      <Show when={restoring()}>
        {(backup) => (
          <Modal
            title="Restaurar este backup?"
            onClose={() => !busy.active() && setRestoring(null)}
            footer={
              <>
                <button type="button" disabled={busy.active()} onClick={() => setRestoring(null)}>
                  Cancelar
                </button>
                <button
                  type="button"
                  class="primary danger"
                  disabled={busy.active()}
                  onClick={() => void restore(backup())}
                >
                  {busy.active() ? "Restaurando…" : "Restaurar"}
                </button>
              </>
            }
          >
            <p>
              Os dados atuais serão substituídos pelos do backup de {dateTime(backup().createdAt)}.
            </p>
            <p class="muted">
              Antes disso, o Gabaritei guarda uma cópia dos dados atuais na mesma pasta, e ela
              também aparece na lista para você voltar atrás.
            </p>
          </Modal>
        )}
      </Show>
    </section>
  );
}

function Content(props: {
  status: BackupStatus;
  busy: boolean;
  onBackupNow: () => void;
  onChooseFolder: () => void;
  onUseDefaultFolder: () => void;
  onRestore: (backup: BackupInfo) => void;
}) {
  return (
    <>
      <p>
        Pasta: <code data-testid="backup-folder">{props.status.folder}</code>
        <Show when={props.status.isDefaultFolder}>
          {" "}
          <span class="muted">(padrão)</span>
        </Show>{" "}
        <button type="button" onClick={() => void openFolder("backups")}>
          Abrir pasta dos backups
        </button>
      </p>
      <Show when={props.status.lastError}>
        {(message) => (
          <p class="error" role="alert">
            O último backup falhou: {message()}
          </p>
        )}
      </Show>
      <p class="actions">
        <button
          type="button"
          class="primary"
          disabled={props.busy}
          onClick={() => props.onBackupNow()}
        >
          Fazer backup agora
        </button>
        <button type="button" disabled={props.busy} onClick={() => props.onChooseFolder()}>
          Escolher pasta…
        </button>
        <Show when={!props.status.isDefaultFolder}>
          <button type="button" disabled={props.busy} onClick={() => props.onUseDefaultFolder()}>
            Usar a pasta padrão
          </button>
        </Show>
      </p>
      <Show
        when={props.status.backups.length > 0}
        fallback={<p class="muted">Nenhum backup nesta pasta ainda.</p>}
      >
        <ul class="backup-list">
          <For each={props.status.backups}>
            {(backup) => (
              <li>
                <span>
                  {dateTime(backup.createdAt)} · {megabytes(backup.sizeBytes)}
                  <Show when={backup.kind !== "backup"}>
                    {" "}
                    <span class="badge">
                      {backup.kind === "safety" ? "antes de restaurar" : "manual"}
                    </span>
                  </Show>
                </span>
                <button
                  type="button"
                  disabled={props.busy}
                  aria-label={`Restaurar o backup de ${dateTime(backup.createdAt)}`}
                  onClick={() => props.onRestore(backup)}
                >
                  Restaurar
                </button>
              </li>
            )}
          </For>
        </ul>
      </Show>
    </>
  );
}
