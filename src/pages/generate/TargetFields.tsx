import { For, Show, createSignal } from "solid-js";

const OTHER = "__other__";

/** Banca, concurso e cargo: what the questions should look like. All optional. */
export default function TargetFields(props: {
  boards: string[];
  board: string | null;
  contest: string | null;
  position: string | null;
  onBoard: (board: string | null) => void;
  onContest: (contest: string | null) => void;
  onPosition: (position: string | null) => void;
}) {
  // "Outra" stays chosen while the user types a board that is not in the list.
  const [other, setOther] = createSignal(false);
  const selected = () => (other() ? OTHER : (props.board ?? ""));
  return (
    <>
      <label for="gen-board">Banca</label>
      <select
        id="gen-board"
        value={selected()}
        onChange={(event) => {
          const value = event.currentTarget.value;
          setOther(value === OTHER);
          props.onBoard(value === OTHER || value === "" ? null : value);
        }}
      >
        <option value="">Opcional: sem banca específica</option>
        <For each={props.boards}>{(board) => <option value={board}>{board}</option>}</For>
        <option value={OTHER}>Outra…</option>
      </select>
      <Show when={other()}>
        <label for="gen-board-other">Nome da banca</label>
        <input
          id="gen-board-other"
          type="text"
          value={props.board ?? ""}
          onInput={(event) => props.onBoard(event.currentTarget.value || null)}
        />
      </Show>

      <label for="gen-contest">Concurso</label>
      <input
        id="gen-contest"
        type="text"
        placeholder="Opcional: ex. TRF 1ª Região 2024"
        value={props.contest ?? ""}
        onInput={(event) => props.onContest(event.currentTarget.value || null)}
      />

      <label for="gen-position">Cargo</label>
      <input
        id="gen-position"
        type="text"
        placeholder="Opcional: ex. Analista Judiciário"
        value={props.position ?? ""}
        onInput={(event) => props.onPosition(event.currentTarget.value || null)}
      />
    </>
  );
}
