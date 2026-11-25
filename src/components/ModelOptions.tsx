import { For, Show, onMount } from "solid-js";
import type { Agent } from "../bindings/Agent";
import type { Effort } from "../bindings/Effort";
import type { RoleConfig } from "../bindings/RoleConfig";
import {
  catalogState,
  effortOptions,
  fastSupport,
  loadCatalog,
  readyList,
  roleForModel,
  unavailableReason,
} from "../catalog";
import { effortLabels } from "../labels";

interface FieldProps {
  /** Names the field for assistive technology, e.g. the role's name. */
  label: string;
  value: RoleConfig;
  onChange: (config: RoleConfig) => void;
}

/**
 * Model of a role: a list from the agent's CLI, or free text when the agent offers no list.
 * A saved model that the list no longer has stays selected, marked as unavailable.
 */
export function ModelField(props: FieldProps) {
  onMount(() => void loadCatalog());
  const list = () => readyList(catalogState(), props.value.agent);
  return (
    <Show
      when={list()}
      fallback={
        <input
          type="text"
          aria-label={`Modelo: ${props.label}`}
          placeholder="Digite o modelo"
          value={props.value.model}
          onInput={(event) => props.onChange({ ...props.value, model: event.currentTarget.value })}
        />
      }
    >
      {(agent) => (
        <select
          aria-label={`Modelo: ${props.label}`}
          onChange={(event) =>
            props.onChange(roleForModel(catalogState(), props.value, event.currentTarget.value))
          }
        >
          <Show when={!agent().models.some((model) => model.id === props.value.model)}>
            <option value={props.value.model} selected>
              {props.value.model ? `${props.value.model} (não disponível)` : "Escolha um modelo"}
            </option>
          </Show>
          <For each={agent().models}>
            {(model) => (
              <option value={model.id} selected={model.id === props.value.model}>
                {model.label}
              </option>
            )}
          </For>
        </select>
      )}
    </Show>
  );
}

/** Effort of a role: the efforts the selected model supports. */
export function EffortField(props: FieldProps) {
  const options = () => effortOptions(catalogState(), props.value);
  return (
    <select
      aria-label={`Esforço: ${props.label}`}
      onChange={(event) =>
        props.onChange({ ...props.value, effort: event.currentTarget.value as Effort })
      }
    >
      <Show when={!options().includes(props.value.effort)}>
        <option value={props.value.effort} selected>
          {effortLabels[props.value.effort]} (não disponível)
        </option>
      </Show>
      <For each={options()}>
        {(effort) => (
          <option value={effort} selected={effort === props.value.effort}>
            {effortLabels[effort]}
          </option>
        )}
      </For>
    </select>
  );
}

/** Fast Mode of a role: shown unless the selected model is known to lack it. */
export function FastField(props: FieldProps) {
  return (
    <Show when={fastSupport(catalogState(), props.value) !== "no"}>
      <label class="check">
        <input
          type="checkbox"
          aria-label={`Modo rápido: ${props.label}`}
          checked={props.value.fast}
          onChange={(event) =>
            props.onChange({ ...props.value, fast: event.currentTarget.checked })
          }
        />
        Modo rápido
      </label>
      <Show when={props.value.agent === "claude"}>
        <p class="muted" role="note">
          Exige uso extra ativado na conta Claude; sem isso, roda no modo normal.
        </p>
      </Show>
    </Show>
  );
}

/** Says why an agent's model list couldn't be read, so its fields are free text. */
export function CatalogNotice(props: { agents: Agent[] }) {
  const reasons = () => [
    ...new Set(
      props.agents.flatMap((agent) => {
        const reason = unavailableReason(catalogState(), agent);
        return reason ? [reason] : [];
      }),
    ),
  ];
  return (
    <For each={reasons()}>
      {(reason) => (
        <p class="muted" role="note">
          {reason}
        </p>
      )}
    </For>
  );
}
