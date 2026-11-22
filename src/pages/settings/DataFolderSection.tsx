import { Show, createResource } from "solid-js";
import { api } from "../../api";
import { ErrorState, Loading } from "../../components/states";
import { openFolder } from "./openFolder";

/** Where the study data lives on this computer, with a button to open it. */
export default function DataFolderSection() {
  const [folder, { refetch }] = createResource(() => api.dataFolder());

  return (
    <section class="card" aria-labelledby="data-folder-title">
      <h2 id="data-folder-title">Pasta de dados</h2>
      <p class="muted">
        O banco de dados e as imagens das questões ficam nesta pasta, neste computador.
      </p>
      <Show
        when={!folder.error}
        fallback={
          <ErrorState
            error={folder.error as unknown}
            onRetry={() => refetch()}
            busy={folder.loading}
            describedBy="data-folder-title"
          />
        }
      >
        <Show when={folder()} fallback={<Loading />}>
          {(path) => (
            <p class="actions">
              <code data-testid="data-folder">{path()}</code>
              <button type="button" onClick={() => void openFolder("data")}>
                Abrir pasta de dados
              </button>
            </p>
          )}
        </Show>
      </Show>
    </section>
  );
}
