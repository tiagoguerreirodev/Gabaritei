import { For } from "solid-js";

const labels = {
  contest: { name: "Concurso", all: "Todos os concursos" },
  board: { name: "Banca", all: "Todas as bancas" },
};

export default function FilterSelect(props: {
  field: keyof typeof labels;
  value: string;
  options: string[];
  onChange: (value: string) => void;
}) {
  return (
    <label>
      <span>{labels[props.field].name}</span>
      <select
        class="input"
        value={props.value}
        onChange={(event) => props.onChange(event.currentTarget.value)}
      >
        <option value="">{labels[props.field].all}</option>
        <For each={props.options}>{(name) => <option value={name}>{name}</option>}</For>
      </select>
    </label>
  );
}
