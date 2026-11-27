import { A } from "@solidjs/router";
import { For, Show } from "solid-js";
import type { FirstStep } from "../../bindings/FirstStep";
import type { FirstStepStatus } from "../../bindings/FirstStepStatus";

const steps: Record<FirstStep, { label: string; href: string; link: string }> = {
  checkAgents: {
    label: "Confira os agentes",
    href: "/configuracoes",
    link: "Abrir Configurações › Agentes",
  },
  generatePackage: { label: "Gere o primeiro pacote", href: "/gerar", link: "Gerar questões" },
  reviewPackage: { label: "Revise o pacote", href: "/revisar-pacote", link: "Revisar pacote" },
  finishAttempt: { label: "Resolva a primeira tentativa", href: "/banco", link: "Ir para o Banco" },
};

/**
 * The first-use checklist on Início. It never blocks the page: it sits above the day's main
 * action and disappears once the backend reports every step done (an empty list).
 */
export default function FirstSteps(props: { steps: FirstStepStatus[] }) {
  const doneCount = () => props.steps.filter((status) => status.done).length;

  const currentIndex = () => props.steps.findIndex((status) => !status.done);

  return (
    <Show when={props.steps.length > 0}>
      <section class="card card-wide first-steps" aria-labelledby="first-steps-title">
        <h2 id="first-steps-title">Primeiros passos</h2>
        <p class="muted">
          {doneCount()} de {props.steps.length} concluídos
        </p>
        <ol class="steps" role="list">
          <For each={props.steps}>
            {(status, index) => (
              <li
                classList={{ done: status.done, current: index() === currentIndex() }}
                aria-current={index() === currentIndex() ? "step" : undefined}
              >
                {steps[status.step].label}
                <span class="sr-only">
                  {status.done
                    ? " (concluída)"
                    : index() === currentIndex()
                      ? " (em andamento)"
                      : " (pendente)"}
                </span>
                <Show when={!status.done}>
                  <A href={steps[status.step].href}>{steps[status.step].link}</A>
                </Show>
              </li>
            )}
          </For>
        </ol>
      </section>
    </Show>
  );
}
