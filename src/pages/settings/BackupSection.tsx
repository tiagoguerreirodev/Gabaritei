import { For, Show, createResource, createSignal } from "solid-js";
import { api } from "../../api";
import type { BackupInfo } from "../../bindings/BackupInfo";
import type { BackupStatus } from "../../bindings/BackupStatus";
import type { LeftBehind } from "../../bindings/LeftBehind";
import Modal from "../../components/Modal";
import { ErrorState, Loading } from "../../components/states";
import { showError, showToast } from "../../components/toasts";

const dateTime = (iso: string) =>
  new Date(iso).toLocaleString("pt-BR", { dateStyle: "short", timeStyle: "short" });

const megabytes = (bytes: number) =>
  `${(bytes / (1024 * 1024)).toLocaleString("pt-BR", { maximumFractionDigits: 1 })} MB`;

/** Backups of the study data: the folder, a backup now, and restore. */
export default function BackupSection(props: {
  /** Called after a restore replaced the data, so the app shows the restored state. */
  onRestored?: () => void;
}) {
  // Read once: the callback is a plain function, not something that changes.
  // eslint-disable-next-line solid/reactivity
  const onRestored = props.onRestored;
  const [status, { mutate, refetch }] = createResource(() => api.backupStatus());
  const [busy, setBusy] = createSignal(false);
  const [restoring, setRestoring] = createSignal<BackupInfo | null>(null);
  // What the last folder change left in the old folder; kept until dismissed or changed again.
  const [leftBehind, setLeftBehind] = createSignal<LeftBehind | null>(null);

  const run = async (work: () => Promise<void>) => {
    setBusy(true);
    try {
      await work();
    } catch (error) {
      showError(error);
      // The failure may have been recorded (a backup that failed), so show it.
      void refetch();
    } finally {
      setBusy(false);
    }
  };

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
        setLeftBehind(chosen.leftBehind);
        showToast("Pasta dos backups salva.");
      }
    });

  const useDefaultFolder = () =>
    run(async () => {
      const status = await api.backupUseDefaultFolder();
      mutate(status);
      setLeftBehind(status.leftBehind);
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
      <p class="muted">
        <strong>Não use a mesma pasta nos dois computadores</strong> (por exemplo, uma pasta
        sincronizada na nuvem): cada computador apaga os backups mais antigos além do limite, e os
        do outro computador entram nessa conta e se misturam na lista.
      </p>
      <Show when={leftBehind()}>
        {(left) => (
          <div class="notice" role="status" data-testid="left-behind">
            <p>
              A pasta anterior (<code>{left().folder}</code>) ainda tem{" "}
              {left().backups === 1
                ? "1 backup, que não aparece mais aqui."
                : `${left().backups} backups, que não aparecem mais aqui.`}
              <Show when={left().safety > 0}>
                {" "}
                {left().safety === 1
                  ? "Um deles é uma cópia feita antes de restaurar ou importar"
                  : `${left().safety} deles são cópias feitas antes de restaurar ou importar`}
                ; para voltar atrás, volte para aquela pasta.
              </Show>{" "}
              Nada foi movido nem apagado. Se quiser, copie os backups para a pasta nova.
            </p>
            <button type="button" onClick={() => setLeftBehind(null)}>
              Entendi
            </button>
          </div>
        )}
      </Show>
      <Show when={!status.error} fallback={<ErrorState error={status.error as unknown} />}>
        <Show when={status()} fallback={<Loading />}>
          {(current) => (
            <Content
              status={current()}
              busy={busy()}
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
            onClose={() => setRestoring(null)}
            footer={
              <>
                <button type="button" onClick={() => setRestoring(null)}>
                  Cancelar
                </button>
                <button
                  type="button"
                  class="primary danger"
                  disabled={busy()}
                  onClick={() => void restore(backup())}
                >
                  {busy() ? "Restaurando…" : "Restaurar"}
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
        </Show>
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
