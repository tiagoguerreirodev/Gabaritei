import { formatDateTime, formatMegabytes } from "../../format";
import { For, Show, createResource, createSignal } from "solid-js";
import { api } from "../../api";
import type { BackupInfo } from "../../bindings/BackupInfo";
import type { BackupKind } from "../../bindings/BackupKind";
import type { BackupListing } from "../../bindings/BackupListing";
import type { BackupStatus } from "../../bindings/BackupStatus";
import type { LeftBehind } from "../../bindings/LeftBehind";
import type { RelocateOutcome } from "../../bindings/RelocateOutcome";
import type { RelocateReport } from "../../bindings/RelocateReport";
import ConfirmModal from "../../components/ConfirmModal";
import { InlineError, ResourceView } from "../../components/states";
import { showSuccess } from "../../components/toasts";
import type { ActionRunner } from "../../actions";
import { openFolder } from "./openFolder";
import LeftBehindNotice from "./LeftBehindNotice";

/** Backups of the study data: the folder, a backup now, and restore. */
export default function BackupSection(props: {
  /** Shared with the data-file section, so only one action runs at a time. */
  busy: ActionRunner;
  /** Called after a restore replaced the data, so the app shows the restored state. */
  onRestored?: () => void;
}) {
  // Read once: these are plain objects and functions, not something that changes.
  // eslint-disable-next-line solid/reactivity
  const { busy, onRestored } = props;
  // What a folder change left in the old folder is remembered by the backend, so it is shown
  // again after a restart; the first load seeds it.
  const [leftBehind, setLeftBehind] = createSignal<LeftBehind | null>(null);
  const [status, { mutate, refetch }] = createResource(async () => {
    const loaded = await api.backupStatus();
    setLeftBehind(loaded.leftBehind);
    return loaded;
  });
  const [restoring, setRestoring] = createSignal<BackupInfo | null>(null);
  const [confirmingMove, setConfirmingMove] = createSignal(false);
  // Set once a copy or move ran, so the notice stops saying nothing was moved.
  const [relocated, setRelocated] = createSignal(false);

  const run = (work: () => Promise<void>) =>
    busy.run(work, () => {
      // The failure may have been recorded (a backup that failed), so show it.
      void refetch();
    });

  const backupNow = () =>
    run(async () => {
      await api.backupNow();
      showSuccess("Backup feito.");
      mutate(await api.backupStatus());
    });

  const chooseFolder = () =>
    run(async () => {
      const chosen = await api.backupChooseFolder();
      if (chosen) {
        mutate(chosen);
        setLeftBehind(chosen.leftBehind);
        setRelocated(false);
        showSuccess("Pasta dos backups salva.");
      }
    });

  const useDefaultFolder = () =>
    run(async () => {
      const status = await api.backupUseDefaultFolder();
      mutate(status);
      setLeftBehind(status.leftBehind);
      setRelocated(false);
      showSuccess("Voltou para a pasta padrão.");
    });

  const relocate = (operation: () => Promise<RelocateOutcome>, verb: "copiado" | "movido") =>
    run(async () => {
      const { report, status: next } = await operation();
      mutate(next);
      setLeftBehind(next.leftBehind);
      setRelocated(true);
      showSuccess(relocateSummary(report, verb));
    });

  const copyBackups = () => relocate(api.backupCopyLeftBehind, "copiado");
  const moveBackups = () => relocate(api.backupMoveLeftBehind, "movido");

  const restore = (backup: BackupInfo) =>
    run(async () => {
      await api.backupRestore(backup.name);
      setRestoring(null);
      showSuccess("Dados restaurados.");
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
        Os dois computadores podem usar a mesma pasta (por exemplo, uma pasta sincronizada na
        nuvem): cada um apaga só os próprios backups mais antigos, e os do outro aparecem na lista
        com o nome do computador. Atualize o Gabaritei nos dois antes de compartilhar a pasta.
      </p>
      <Show when={leftBehind()}>
        {(left) => (
          <LeftBehindNotice
            offer={left()}
            relocated={relocated()}
            busy={busy.active()}
            onCopy={() => void copyBackups()}
            onMove={() => setConfirmingMove(true)}
            onDismiss={() =>
              void run(async () => {
                await api.backupForgetLeftBehind();
                setLeftBehind(null);
              })
            }
          />
        )}
      </Show>
      <ResourceView
        value={status()}
        error={() => status.error}
        pending={status.state === "pending"}
        retrying={status.loading}
        onRetry={() => refetch()}
        describedBy="backup-title"
      >
        {(current) => (
          <Content
            status={current()}
            busy={busy.active()}
            onBackupNow={() => void backupNow()}
            onChooseFolder={() => void chooseFolder()}
            onUseDefaultFolder={() => void useDefaultFolder()}
            onRefresh={() => void refetch()}
            refreshing={status.loading}
            onRestore={setRestoring}
          />
        )}
      </ResourceView>
      <Show when={confirmingMove()}>
        <ConfirmModal
          title="Mover os backups para a pasta nova?"
          confirmLabel="Mover"
          danger
          busy={busy.active()}
          dismissBlocked={busy.active()}
          onClose={() => setConfirmingMove(false)}
          onConfirm={() => {
            setConfirmingMove(false);
            void moveBackups();
          }}
        >
          <p>
            Cada backup é copiado para a pasta nova e conferido; só então o original é apagado da
            pasta anterior (os originais serão apagados). Se algum não puder ser conferido, ele
            continua onde estava.
          </p>
          <p class="muted">
            Se preferir manter os backups nas duas pastas, use <strong>Copiar</strong>.
          </p>
        </ConfirmModal>
      </Show>
      <Show when={restoring()}>
        {(backup) => (
          <ConfirmModal
            title="Restaurar este backup?"
            confirmLabel="Restaurar"
            busyLabel="Restaurando…"
            danger
            busy={busy.active()}
            dismissBlocked={busy.active()}
            onClose={() => setRestoring(null)}
            onConfirm={() => void restore(backup())}
          >
            <p>
              Os dados atuais serão substituídos pelos do backup de{" "}
              {formatDateTime(backup().createdAt)}.
            </p>
            <p class="muted">
              Antes disso, o Gabaritei guarda uma cópia dos dados atuais na mesma pasta, e ela
              também aparece na lista para você voltar atrás.
            </p>
          </ConfirmModal>
        )}
      </Show>
    </section>
  );
}

/** Formats the outcome of a backup relocation in pt-BR. */
export function relocateSummary(report: RelocateReport, verb: "copiado" | "movido"): string {
  const done = report.copied + report.alreadyThere;
  const parts: string[] = [];
  if (done > 0 || report.failed === 0) {
    parts.push(`${backupCount(done)} ${verb}${done === 1 ? "" : "s"} para a pasta nova`);
  }
  if (report.failed > 0) {
    const action = report.failed === 1 ? `foi ${verb}` : `foram ${verb}s`;
    const failure = report.firstFailure ? ` (${report.firstFailure})` : "";
    parts.push(
      `${backupCount(report.failed)} não ${action}${failure}; os originais continuam na pasta anterior`,
    );
  }
  if (report.leftAlone > 0) {
    const action = report.leftAlone === 1 ? "ficou onde estava" : "ficaram onde estavam";
    parts.push(
      `${backupCount(report.leftAlone)} de outro computador (ou sem identificação) ${action}`,
    );
  }
  return `${parts.join(". ")}.`;
}

function backupCount(count: number): string {
  return `${count} ${count === 1 ? "backup" : "backups"}`;
}

const backupKindBadge: Record<BackupKind, string | undefined> = {
  backup: undefined,
  safety: "antes de restaurar",
  manual: "manual",
};

const listingNotices: Record<
  Exclude<BackupListing["state"], "ready">,
  { id: string; text: string }
> = {
  folderUnavailable: {
    id: "folder-unavailable",
    text: "A pasta dos backups não está acessível agora (um disco desconectado ou uma pasta de rede fora do ar, por exemplo). Os backups dela não aparecem até ela voltar; nada foi apagado. Conecte-a de novo ou escolha outra pasta.",
  },
  slow: {
    id: "backup-list-slow",
    text: "A leitura da lista de backups está demorando. Você ainda pode fazer backups nesta pasta.",
  },
};

function Content(props: {
  status: BackupStatus;
  busy: boolean;
  onBackupNow: () => void;
  onChooseFolder: () => void;
  onUseDefaultFolder: () => void;
  onRefresh: () => void;
  /** The status is being read again. */
  refreshing: boolean;
  onRestore: (backup: BackupInfo) => void;
}) {
  const folderUnavailable = () => props.status.listing.state === "folderUnavailable";
  const notice = () => {
    const { state } = props.status.listing;
    return state === "ready" ? undefined : listingNotices[state];
  };
  const backups = () => {
    const listing = props.status.listing;
    return listing.state === "ready" ? listing.backups : undefined;
  };
  return (
    <>
      <p>
        Pasta: <code data-testid="backup-folder">{props.status.folder}</code>
        <Show when={props.status.isDefaultFolder}>
          {" "}
          <span class="muted">(padrão)</span>
        </Show>{" "}
        <button type="button" class="btn" onClick={() => void openFolder("backups")}>
          Abrir pasta dos backups
        </button>
      </p>
      <Show when={!folderUnavailable() && props.status.lastError}>
        {(message) => <InlineError>O último backup falhou: {message()}</InlineError>}
      </Show>
      <p class="actions">
        <button type="button" class="btn" disabled={props.busy} onClick={() => props.onBackupNow()}>
          Fazer backup agora
        </button>
        <button
          type="button"
          class="btn"
          disabled={props.busy}
          onClick={() => props.onChooseFolder()}
        >
          Escolher pasta…
        </button>
        <Show when={!props.status.isDefaultFolder}>
          <button
            type="button"
            class="btn"
            disabled={props.busy}
            onClick={() => props.onUseDefaultFolder()}
          >
            Usar a pasta padrão
          </button>
        </Show>
      </p>
      <Show when={notice()}>
        {(current) => (
          <p class="card card-row notice" role="status" data-testid={current().id}>
            {current().text}{" "}
            <button
              type="button"
              class="btn"
              disabled={props.busy || props.refreshing}
              onClick={() => props.onRefresh()}
            >
              Verificar de novo
            </button>
          </p>
        )}
      </Show>
      <Show
        when={backups()?.length}
        fallback={
          <Show when={props.status.listing.state === "ready"}>
            <p class="muted">Nenhum backup nesta pasta ainda.</p>
          </Show>
        }
      >
        <ul class="settings-list">
          <For each={backups()}>
            {(backup) => (
              <li>
                <span>
                  {formatDateTime(backup.createdAt)} · {formatMegabytes(backup.sizeBytes)}
                  <Show when={backup.otherComputer}>
                    {" "}
                    <span class="badge">outro computador ({backup.computer})</span>
                  </Show>
                  <Show when={backupKindBadge[backup.kind]}>
                    {(label) => (
                      <>
                        {" "}
                        <span class="badge">{label()}</span>
                      </>
                    )}
                  </Show>
                </span>
                <button
                  type="button"
                  class="btn"
                  disabled={props.busy}
                  aria-label={`Restaurar o backup de ${formatDateTime(backup.createdAt)}`}
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
