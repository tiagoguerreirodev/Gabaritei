import { Show } from "solid-js";
import { api } from "../../api";
import type { GenerateSources } from "../../bindings/GenerateSources";
import { showError } from "../../components/toasts";
import Optional from "./Optional";
import AttachmentList from "./AttachmentList";

export default function SourcesFields(props: {
  value: GenerateSources;
  remnoteOn: boolean;
  onChange: (sources: GenerateSources) => void;
}) {
  const attach = async () => {
    try {
      const picked = await api.pickAttachments();
      const current = props.value.attachments;
      props.onChange({ ...props.value, attachments: [...new Set([...current, ...picked])] });
    } catch (error) {
      showError(error);
    }
  };
  return (
    <div class="field">
      <span class="field-label">
        Fontes <Optional />
      </span>
      <div>
        <label>
          <input
            type="checkbox"
            checked={props.value.remnote}
            disabled={!props.remnoteOn}
            onChange={(event) =>
              props.onChange({ ...props.value, remnote: event.currentTarget.checked })
            }
          />{" "}
          RemNote
        </label>
        <Show when={!props.remnoteOn}>
          <span class="muted"> (ligue em Configurações)</span>
        </Show>
        <br />
        <label>
          <input
            type="checkbox"
            checked={props.value.web}
            onChange={(event) =>
              props.onChange({ ...props.value, web: event.currentTarget.checked })
            }
          />{" "}
          Pesquisar na web
        </label>
        <button type="button" class="btn" onClick={() => void attach()}>
          Anexar arquivos
        </button>
        <AttachmentList
          paths={props.value.attachments}
          styleReferences={props.value.styleReferences}
          onToggleStyle={(path, isStyle) =>
            props.onChange({
              ...props.value,
              styleReferences: isStyle
                ? [...props.value.styleReferences, path]
                : props.value.styleReferences.filter((p) => p !== path),
            })
          }
          onRemove={(path) =>
            props.onChange({
              ...props.value,
              attachments: props.value.attachments.filter((p) => p !== path),
              styleReferences: props.value.styleReferences.filter((p) => p !== path),
            })
          }
        />
      </div>
    </div>
  );
}
