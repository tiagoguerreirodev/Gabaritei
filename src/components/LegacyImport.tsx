import { formatDate, formatNumber } from "../format";
import { Match, Show, Switch, createSignal } from "solid-js";
import { api } from "../api";
import type { LegacyImportReport } from "../bindings/LegacyImportReport";
import type { Busy } from "../pages/settings/busy";
import { errorMessage, InlineError } from "./states";
import { showToast } from "./toasts";

export function reportSummary(report: LegacyImportReport) {
  return (
    `${formatNumber(report.questions)} questões, ${formatNumber(report.collections)} cadernos, ` +
    `${formatNumber(report.topics)} assuntos e ${formatNumber(report.images)} imagens`
  );
}

/**
 * Imports the legacy Caderno Local bank. Shows the import button until it has run,
 * then the summary.
 */
export default function LegacyImport(props: {
  /** Report of a previous import; the component tracks later imports itself. */
  initialReport: LegacyImportReport | null;
  onImported?: (report: LegacyImportReport) => void;
  /**
   * The shared "something is running" flag of Configurações → Dados. When given, the import
   * cannot start while a backup, restore, export or data-file import runs, and they cannot
   * start while this one runs.
   */
  busy?: Busy;
}) {
  const [report, setReport] = createSignal(props.initialReport);
  const [importing, setImporting] = createSignal(false);
  const [error, setError] = createSignal("");
  const blocked = () => importing() || (props.busy?.active() ?? false);

  const run = async () => {
    setImporting(true);
    setError("");
    try {
      const outcome = await api.importLegacyBank();
      if (outcome) {
        setReport(outcome.report);
        showToast("Banco do Caderno Local importado.");
        props.onImported?.(outcome.report);
      }
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setImporting(false);
    }
  };

  const start = async () => {
    if (props.busy) await props.busy.run(run);
    else await run();
  };

  return (
    <section class="card" aria-labelledby="legacy-import-title">
      <h2 id="legacy-import-title">Banco do Caderno Local</h2>
      <Switch>
        <Match when={report()}>
          {(done) => (
            <p data-testid="legacy-import-summary">
              Importado em {formatDate(done().importedAt)}: {reportSummary(done())}.
            </p>
          )}
        </Match>
        <Match when={!report()}>
          <p class="muted">
            Traga as questões, os cadernos e as imagens do aplicativo antigo. Escolha a pasta do
            repositório Simulados-Concursos neste computador. Os arquivos são conferidos antes de
            qualquer alteração, e a importação acontece uma única vez.
          </p>
          <button type="button" class="primary" disabled={blocked()} onClick={() => void start()}>
            {importing() ? "Importando…" : "Escolher pasta e importar"}
          </button>
        </Match>
      </Switch>
      <Show when={error()}>
        <InlineError>{error()}</InlineError>
      </Show>
    </section>
  );
}
