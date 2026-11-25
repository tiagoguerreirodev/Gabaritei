import type { RoleEditor } from "../../catalog";
import { AgentField, EffortField, FastField, ModelField } from "../../components/ModelOptions";

/** Agent, model and effort for one role, for this generation only. */
export default function RolePicker(props: { label: string; editor: RoleEditor }) {
  return (
    <div class="role-picker" role="group" aria-label={props.label}>
      <AgentField label={props.label} editor={props.editor} />
      <ModelField label={props.label} editor={props.editor} />
      <EffortField label={props.label} editor={props.editor} />
      <FastField label={props.label} editor={props.editor} />
    </div>
  );
}
