import { A } from "@solidjs/router";
import { Match, Show, Switch } from "solid-js";
import { dismissGeneration, generationStatus, waitingDrafts } from "../generation";

/**
 * Shows, on any page, that a package is being generated, how a failed or cancelled run ended,
 * and, for as long as any package waits for review, a link to review it. That last notice does
 * not depend on the generation outcome, so opening the review or leaving the page never
 * removes it: only importing or discarding the package does.
 */
export default function GenerationBanner() {
  return (
    <>
      <WaitingNotice />
      <RunStatus />
    </>
  );
}

function WaitingNotice() {
  const count = () => waitingDrafts().length;
  return (
    <Show when={!generationStatus()?.running && count() > 0}>
      <p class="generation-banner" role="status">
        <span>
          {count() === 1
            ? "1 pacote esperando revisão."
            : `${count().toLocaleString("pt-BR")} pacotes esperando revisão.`}
        </span>
        <A
          href={
            count() === 1
              ? `/revisar-pacote?id=${encodeURIComponent(waitingDrafts()[0]?.id ?? "")}`
              : "/revisar-pacote"
          }
        >
          {count() === 1 ? "Revisar pacote" : "Revisar pacotes"}
        </A>
      </p>
    </Show>
  );
}

function RunStatus() {
  return (
    <Show when={generationStatus()}>
      {(status) => (
        <Switch>
          <Match when={status().running}>
            <p class="generation-banner" role="status">
              <span>
                Gerando um pacote… <span class="muted">{status().progress?.label ?? ""}</span>
              </span>
              <A href="/gerar">Acompanhar</A>
            </p>
          </Match>
          <Match when={status().outcome?.kind !== "draft" && status().outcome}>
            {(outcome) => (
              <p class="generation-banner" role="status">
                <Switch>
                  <Match when={outcome().kind === "failed"}>
                    <span class="error">
                      A geração falhou: {(outcome() as { kind: "failed"; message: string }).message}
                    </span>
                  </Match>
                  <Match when={outcome().kind === "cancelled"}>
                    <span>Geração cancelada.</span>
                  </Match>
                </Switch>
                <button type="button" onClick={() => void dismissGeneration()}>
                  Dispensar
                </button>
              </p>
            )}
          </Match>
        </Switch>
      )}
    </Show>
  );
}
