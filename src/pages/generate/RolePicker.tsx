import { For } from "solid-js";
import type { Agent } from "../../bindings/Agent";
import type { RoleConfig } from "../../bindings/RoleConfig";
import { catalogState, roleForAgent } from "../../catalog";
import { EffortField, FastField, ModelField } from "../../components/ModelOptions";
import { agentLabels, agents } from "../../labels";

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
          props.onChange(
            roleForAgent(catalogState(), event.currentTarget.value as Agent, props.value),
          )
        }
      >
        <For each={agents}>{(agent) => <option value={agent}>{agentLabels[agent]}</option>}</For>
      </select>
      <ModelField label={props.label} value={props.value} onChange={props.onChange} />
      <EffortField label={props.label} value={props.value} onChange={props.onChange} />
      <FastField label={props.label} value={props.value} onChange={props.onChange} />
    </div>
  );
}
