import { A } from "@solidjs/router";
import { Match, Show, Switch } from "solid-js";
import { dismissGeneration, generationStatus } from "../generation";

/**
 * Shows, on any page, that a package is being generated, and afterwards how it ended. Hidden
 * on the Gerar page itself, which shows the same information in full.
 */
export default function GenerationBanner() {
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
          <Match when={status().outcome}>
            {(outcome) => (
              <p class="generation-banner" role="status">
                <Switch>
                  <Match when={outcome().kind === "draft"}>
                    <span>O pacote ficou pronto.</span>
                    <A
                      href={`/revisar-pacote?id=${encodeURIComponent(
                        (outcome() as { kind: "draft"; draftId: string }).draftId,
                      )}`}
                      onClick={() => void dismissGeneration()}
                    >
                      Revisar pacote
                    </A>
                  </Match>
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
