import { For, Show } from "solid-js";

const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

export default function AttachmentList(props: {
  paths: string[];
  styleReferences: string[];
  onToggleStyle: (path: string, isStyle: boolean) => void;
  onRemove: (path: string) => void;
}) {
  return (
    <Show when={props.paths.length > 0}>
      <ul class="attachments" aria-label="Anexos">
        <For each={props.paths}>
          {(path) => (
            <li>
              {fileName(path)}{" "}
              <label>
                <input
                  type="checkbox"
                  aria-label={`Referência de estilo: ${fileName(path)}`}
                  checked={props.styleReferences.includes(path)}
                  onChange={(event) => props.onToggleStyle(path, event.currentTarget.checked)}
                />{" "}
                prova de referência de estilo
              </label>{" "}
              <button
                type="button"
                class="btn btn-link is-inline"
                aria-label={`Remover ${fileName(path)}`}
                onClick={() => props.onRemove(path)}
              >
                remover
              </button>
            </li>
          )}
        </For>
      </ul>
    </Show>
  );
}
