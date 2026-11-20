import { Show, createResource } from "solid-js";
import { api } from "../api";
import LegacyImport from "../components/LegacyImport";
import Placeholder from "../components/Placeholder";

export default function Home() {
  const [status] = createResource(() => api.legacyImportStatus());

  return (
    <Placeholder
      title="Início"
      description="Resumo do seu desempenho, tentativas para retomar, revisões do dia e pacotes aguardando avaliação."
    >
      {/* First run: offer the legacy import while the bank is empty and it hasn't run. */}
      <Show when={status()?.bankEmpty && !status()?.report}>
        <LegacyImport initialReport={null} />
      </Show>
    </Placeholder>
  );
}
