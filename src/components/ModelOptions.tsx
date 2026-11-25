import { For, Show, onMount } from "solid-js";
import type { Agent } from "../bindings/Agent";
import type { Effort } from "../bindings/Effort";
import {
  catalogState,
  loadCatalog,
  readyList,
  unavailableReason,
  type RoleEditor,
} from "../catalog";
import { agentLabels, agents, effortLabels } from "../pages/settings/labels";

interface FieldProps {
  /** Names the field for assistive technology, e.g. the role's name. */
  label: string;
  editor: RoleEditor;
}

export function AgentField(props: FieldProps) {
  return (
    <select
      aria-label={`Agente: ${props.label}`}
      disabled={props.editor.changing()}
      onChange={(event) => {
        const agent = event.currentTarget.value as Agent;
        event.currentTarget.value = props.editor.value().agent;
        void props.editor.change({ kind: "agent", agent });
      }}
    >
      <For each={agents}>
        {(agent) => (
          <option value={agent} selected={agent === props.editor.value().agent}>
            {agentLabels[agent]}
          </option>
        )}
      </For>
    </select>
  );
}

/** Model of a role: a list from the agent's CLI, or free text when the agent offers no list. */
export function ModelField(props: FieldProps) {
  onMount(() => void loadCatalog());
  const value = () => props.editor.value();
  const list = () => readyList(catalogState(), value().agent);
  return (
    <Show
      when={list()}
      fallback={
        <input
          type="text"
          aria-label={`Modelo: ${props.label}`}
          placeholder="Digite o modelo"
          value={value().model}
          disabled={props.editor.changing()}
          onInput={(event) =>
            props.editor.onChange({ ...value(), model: event.currentTarget.value })
          }
        />
      }
    >
      {(agent) => (
        <select
          aria-label={`Modelo: ${props.label}`}
          disabled={props.editor.changing()}
          onChange={(event) => {
            const model = event.currentTarget.value;
            event.currentTarget.value = value().model;
            void props.editor.change({ kind: "model", model });
          }}
        >
          <Show when={!agent().models.some((model) => model.id === value().model)}>
            <option value={value().model} selected>
              {value().model ? `${value().model} (não disponível)` : "Escolha um modelo"}
            </option>
          </Show>
          <For each={agent().models}>
            {(model) => (
              <option value={model.id} selected={model.id === value().model}>
                {model.label}
              </option>
            )}
          </For>
        </select>
      )}
    </Show>
  );
}

export function EffortField(props: FieldProps) {
  const value = () => props.editor.value();
  const options = () => props.editor.options()?.efforts ?? [value().effort];
  return (
    <select
      aria-label={`Esforço: ${props.label}`}
      disabled={props.editor.changing()}
      onChange={(event) =>
        props.editor.onChange({ ...value(), effort: event.currentTarget.value as Effort })
      }
    >
      <Show when={!options().includes(value().effort)}>
        <option value={value().effort} selected>
          {effortLabels[value().effort]} (não disponível)
        </option>
      </Show>
      <For each={options()}>
        {(effort) => (
          <option value={effort} selected={effort === value().effort}>
            {effortLabels[effort]}
          </option>
        )}
      </For>
    </select>
  );
}

export function FastField(props: FieldProps) {
  const value = () => props.editor.value();
  return (
    <Show when={props.editor.options()?.showFast !== false}>
      <label class="check">
        <input
          type="checkbox"
          aria-label={`Modo rápido: ${props.label}`}
          checked={value().fast}
          disabled={props.editor.changing()}
          onChange={(event) =>
            props.editor.onChange({ ...value(), fast: event.currentTarget.checked })
          }
        />
        Modo rápido
      </label>
      <Show when={value().agent === "claude"}>
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
