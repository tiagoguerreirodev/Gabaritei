import { For } from "solid-js";
import type { StatusCount } from "../../bindings/StatusCount";
import type { StudyStatus } from "../../bindings/StudyStatus";
import { formatNumber } from "../../format";
import { studyStatusLabels } from "../../labels";
import { studyStatuses } from "./filterUrl";

/** The five study statuses with how many questions each one would give. */
export default function StatusFilter(props: {
  counts: StatusCount[];
  selected: StudyStatus[];
  onChange: (statuses: StudyStatus[]) => void;
}) {
  const questions = (status: StudyStatus) =>
    props.counts.find((count) => count.status === status)?.questions ?? 0;
  const toggle = (status: StudyStatus, checked: boolean) =>
    props.onChange(
      studyStatuses.filter((s) => (s === status ? checked : props.selected.includes(s))),
    );

  return (
    <fieldset class="filter-group">
      <legend class="label">Status</legend>
      <For each={studyStatuses}>
        {(status) => (
          <label class="check-item">
            <input
              type="checkbox"
              checked={props.selected.includes(status)}
              onChange={(event) => toggle(status, event.currentTarget.checked)}
            />
            <span class="check-label">{studyStatusLabels[status]}</span>
            <span class="check-count">{formatNumber(questions(status))}</span>
          </label>
        )}
      </For>
    </fieldset>
  );
}
