import { For, Show } from "solid-js";

const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

export default function AttachmentList(props: {
  paths: string[];
  onRemove: (path: string) => void;
}) {
  return (
    <Show when={props.paths.length > 0}>
      <ul class="attachments" aria-label="Anexos">
        <For each={props.paths}>
          {(path) => (
            <li>
              {fileName(path)}{" "}
              <button
                type="button"
                class="link-button"
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
