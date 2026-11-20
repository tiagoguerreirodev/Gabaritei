import { For } from "solid-js";
import type { Agent } from "../../bindings/Agent";
import type { Effort } from "../../bindings/Effort";
import type { RoleConfig } from "../../bindings/RoleConfig";
import { agentLabels, agents, effortLabels, efforts } from "../settings/labels";

/** Agent, model and effort for one role, for this generation only. */
export default function RolePicker(props: {
  label: string;
  value: RoleConfig;
  onChange: (config: RoleConfig) => void;
}) {
  return (
    <div class="role-picker" role="group" aria-label={props.label}>
      <select
        aria-label={`Agente: ${props.label}`}
        value={props.value.agent}
        onChange={(event) =>
          props.onChange({ ...props.value, agent: event.currentTarget.value as Agent })
        }
      >
        <For each={agents}>{(agent) => <option value={agent}>{agentLabels[agent]}</option>}</For>
      </select>
      <input
        type="text"
        aria-label={`Modelo: ${props.label}`}
        value={props.value.model}
        onInput={(event) => props.onChange({ ...props.value, model: event.currentTarget.value })}
      />
      <select
        aria-label={`Esforço: ${props.label}`}
        value={props.value.effort}
        onChange={(event) =>
          props.onChange({ ...props.value, effort: event.currentTarget.value as Effort })
        }
      >
        <For each={efforts}>
          {(effort) => <option value={effort}>{effortLabels[effort]}</option>}
        </For>
      </select>
    </div>
  );
}
