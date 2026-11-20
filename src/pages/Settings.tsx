import { Show, createResource } from "solid-js";
import { api } from "../api";
import LegacyImport from "../components/LegacyImport";
import Placeholder from "../components/Placeholder";

export default function Settings() {
  const [status] = createResource(() => api.legacyImportStatus());

  return (
    <Placeholder
      title="Configurações"
      description="Agentes de IA (Claude e Codex), modelos de cada função, RemNote, backups e exportação de dados."
    >
      <h2>Dados</h2>
      <Show when={status()} fallback={<p class="muted">Carregando…</p>}>
        {(current) => <LegacyImport initialReport={current().report} />}
      </Show>
    </Placeholder>
  );
}
