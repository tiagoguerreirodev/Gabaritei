import { For, Show, onCleanup, onMount } from "solid-js";
import {
  generationStage,
  generationStageSentence,
  generationStages,
  generationStatus,
  setGenerationPanelShown,
} from "../generation";
import CancelGeneration from "./CancelGeneration";

/**
 * The stages of a running generation, with the current one marked by text and a shape (not
 * colour alone), and a Cancelar button. It shows only the stages the backend reports.
 */
export default function GenerationProgress() {
  onMount(() => setGenerationPanelShown(true));
  onCleanup(() => setGenerationPanelShown(false));
  return (
    <div class="generation-progress">
      <p role="status">
        <Show when={generationStageSentence()}>{(sentence) => <>{sentence()}…</>}</Show>
        <Show when={generationStatus()?.progress?.label}>
          {(label) => <span class="muted"> {label()}</span>}
        </Show>
      </p>
      <ol class="steps" role="list" aria-label="Etapas da geração">
        <For each={generationStages()}>
          {(stage, index) => (
            <li
              classList={{
                done: index() < (generationStage() ?? -1),
                current: index() === generationStage(),
              }}
              aria-current={index() === generationStage() ? "step" : undefined}
            >
              {stage.label}
              <span class="sr-only">
                {index() < (generationStage() ?? -1)
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
      <Show when={generationStatus()?.canCancel}>
        <CancelGeneration />
      </Show>
    </div>
  );
}
