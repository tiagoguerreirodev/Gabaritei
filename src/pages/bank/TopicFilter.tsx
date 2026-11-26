import { For, createEffect } from "solid-js";
import type { TopicCount } from "../../bindings/TopicCount";
import type { TopicId } from "../../bindings/TopicId";
import type { TopicTree } from "../../bindings/TopicTree";
import { formatNumber } from "../../format";

/** A node of the tree: its own name, the topics under it and how many questions they hold. */
interface Node {
  name: string;
  topicIds: TopicId[];
  children: Node[];
}

/** Area › matter › topic with a checkbox at every level; the selection is a list of topic IDs. */
export default function TopicFilter(props: {
  tree: TopicTree;
  counts: TopicCount[];
  selected: TopicId[];
  onChange: (ids: TopicId[]) => void;
}) {
  const questionsByTopic = () => new Map(props.counts.map((c) => [c.topicId, c.questions]));
  const nodes = (): Node[] =>
    props.tree.areas.map((area) => {
      const matters = area.matters.map((matter) => {
        const topics = matter.topics.map((topic) => ({
          name: topic.name,
          topicIds: [topic.topicId],
          children: [],
        }));
        return { name: matter.name, topicIds: topics.flatMap((t) => t.topicIds), children: topics };
      });
      return { name: area.name, topicIds: matters.flatMap((m) => m.topicIds), children: matters };
    });

  const toggle = (node: Node, checked: boolean) => {
    const others = props.selected.filter((id) => !node.topicIds.includes(id));
    props.onChange(checked ? [...others, ...node.topicIds] : others);
  };

  return (
    <fieldset class="filter-group">
      <legend class="label">Assuntos</legend>
      <ul class="check-tree">
        <For each={nodes()}>
          {(area) => (
            <TreeNode
              node={area}
              selected={props.selected}
              questions={questionsByTopic()}
              onToggle={toggle}
            />
          )}
        </For>
      </ul>
    </fieldset>
  );
}

function TreeNode(props: {
  node: Node;
  selected: TopicId[];
  questions: Map<TopicId, number>;
  onToggle: (node: Node, checked: boolean) => void;
}) {
  let input: HTMLInputElement | undefined;
  const checkedCount = () => props.node.topicIds.filter((id) => props.selected.includes(id)).length;
  const allChecked = () =>
    props.node.topicIds.length > 0 && checkedCount() === props.node.topicIds.length;
  const total = () => {
    let sum = 0;
    for (const id of props.node.topicIds) sum += props.questions.get(id) ?? 0;
    return sum;
  };

  createEffect(() => {
    if (input) input.indeterminate = checkedCount() > 0 && !allChecked();
  });

  const row = () => (
    <label class="check-item">
      <input
        ref={input}
        type="checkbox"
        checked={allChecked()}
        disabled={props.node.topicIds.length === 0}
        onChange={(event) => props.onToggle(props.node, event.currentTarget.checked)}
      />
      <span class="check-label">{props.node.name}</span>
      <span class="check-count">{formatNumber(total())}</span>
    </label>
  );

  return (
    <li>
      {props.node.children.length === 0 ? (
        row()
      ) : (
        <details>
          <summary>{row()}</summary>
          <ul>
            <For each={props.node.children}>
              {(child) => (
                <TreeNode
                  node={child}
                  selected={props.selected}
                  questions={props.questions}
                  onToggle={props.onToggle}
                />
              )}
            </For>
          </ul>
        </details>
      )}
    </li>
  );
}
