import { For, Show } from "solid-js";
import { api } from "../api";
import { GENERATION_STAGES, generationStage, generationStatus } from "../generation";

/**
 * The stages of a running generation, with the current one marked by text and a shape (not
 * colour alone), and a Cancelar button. It shows only the stages the backend reports.
 */
export default function GenerationProgress() {
  const current = () => GENERATION_STAGES[generationStage()];
  return (
    <div class="generation-progress">
      <p role="status">
        Etapa {generationStage() + 1} de {GENERATION_STAGES.length}: {current()?.label}…
        <Show when={generationStatus()?.progress?.label}>
          {(label) => <span class="muted"> {label()}</span>}
        </Show>
      </p>
      <ol class="stage-list" aria-label="Etapas da geração">
        <For each={GENERATION_STAGES}>
          {(stage, index) => (
            <li
              classList={{
                done: index() < generationStage(),
                current: index() === generationStage(),
              }}
              aria-current={index() === generationStage() ? "step" : undefined}
            >
              {stage.label}
              <span class="sr-only">
                {index() < generationStage()
                  ? " (concluída)"
                  : index() === generationStage()
                    ? " (em andamento)"
                    : " (pendente)"}
              </span>
            </li>
          )}
        </For>
      </ol>
      <p class="muted">A etapa de resolver contestações só acontece se houver contestações.</p>
      <button type="button" onClick={() => void api.generationCancel()}>
        Cancelar
      </button>
    </div>
  );
}
