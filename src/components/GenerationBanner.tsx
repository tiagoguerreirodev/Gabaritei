import { formatNumber } from "../format";
import { A, useLocation } from "@solidjs/router";
import { Match, Show, Switch } from "solid-js";
import type { GenerationOutcome } from "../bindings/GenerationOutcome";
import {
  cancelledShortMessage,
  dismissGeneration,
  failedPrefix,
  generationPanelShown,
  generationStageSentence,
  generationStatus,
  isRecheckRun,
  waitingDrafts,
} from "../generation";
import CancelGeneration from "./CancelGeneration";

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
      <p class="card card-row" role="status">
        <span>
          {count() === 1
            ? "1 pacote esperando revisão."
            : `${formatNumber(count())} pacotes esperando revisão.`}
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
  const location = useLocation();
  return (
    <Show when={generationStatus()}>
      {(status) => (
        <Switch>
          <Match when={status().running}>
            {/* The Gerar page shows the stages and its own Cancelar while its panel is up. */}
            <Show when={!(location.pathname === "/gerar" && generationPanelShown())}>
              <p class="card card-row">
                <span>
                  {/* Only the stage is announced, not every progress label. */}
                  <span role="status">
                    {isRecheckRun() ? "Conferindo o pacote…" : "Gerando um pacote…"}
                    {generationStageSentence() ? ` ${generationStageSentence()}.` : ""}
                  </span>{" "}
                  <span class="muted">{status().progress?.label ?? ""}</span>
                </span>
                <span class="banner-actions">
                  <Show when={location.pathname !== "/gerar"}>
                    <A href="/gerar">Acompanhar</A>
                  </Show>
                  <Show when={status().canCancel}>
                    <CancelGeneration />
                  </Show>
                </span>
              </p>
            </Show>
          </Match>
          <Match when={bannerOutcome(status().outcome)}>
            {(notice) => (
              <p class="card card-row" role="status">
                {notice()}
                <button type="button" class="btn" onClick={() => void dismissGeneration()}>
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

function bannerOutcome(outcome: GenerationOutcome | null) {
  if (outcome === null) return;
  switch (outcome.kind) {
    case "draft":
      return;
    case "failed":
      return (
        <span class="error">
          {failedPrefix()} {outcome.message}
        </span>
      );
    case "cancelled":
      return <span>{cancelledShortMessage()}</span>;
  }
}
