import { For, Show, createSignal } from "solid-js";
import { api } from "../../api";
import type { Agent } from "../../bindings/Agent";
import type { CliPaths } from "../../bindings/CliPaths";
import type { CliStatus } from "../../bindings/CliStatus";
import { errorMessage } from "../../components/states";
import { agentLabels, agents } from "./labels";

/** Where each CLI is, with a test button for its version and login state. */
export default function AgentsSection(props: {
  paths: CliPaths;
  onChange: (agent: Agent, path: string) => void;
  /** Saves the settings first, so the test uses the path that is on screen. */
  beforeTest: () => Promise<boolean>;
}) {
  const [statuses, setStatuses] = createSignal<Partial<Record<Agent, CliStatus | string>>>({});
  const [testing, setTesting] = createSignal<Agent | null>(null);

  const test = async (agent: Agent) => {
    setTesting(agent);
    try {
      if (!(await props.beforeTest())) return;
      const status = await api.cliTest(agent);
      setStatuses((current) => ({ ...current, [agent]: status }));
    } catch (error) {
      setStatuses((current) => ({ ...current, [agent]: errorMessage(error) }));
    } finally {
      setTesting(null);
    }
  };

  return (
    <section class="card" aria-labelledby="agents-title">
      <h2 id="agents-title">Agentes</h2>
      <p class="muted">
        O Gabaritei usa os programas <code>claude</code> e <code>codex</code> já instalados e com
        login feito neste computador. Deixe o caminho em branco para detectar sozinho.
      </p>
      <For each={agents}>
        {(agent) => {
          const status = () => statuses()[agent];
          return (
            <div class="settings-row">
              <label for={`cli-${agent}`}>{agentLabels[agent]}</label>
              <input
                id={`cli-${agent}`}
                type="text"
                placeholder="Detectar automaticamente"
                value={props.paths[agent] ?? ""}
                onInput={(event) => props.onChange(agent, event.currentTarget.value)}
              />
              <button type="button" disabled={testing() !== null} onClick={() => void test(agent)}>
                {testing() === agent ? "Testando…" : "Testar"}
              </button>
              <Show when={status()}>
                {(current) => {
                  const text = current();
                  return typeof text === "string" ? (
                    <p class="error settings-status" role="alert">
                      {text}
                    </p>
                  ) : (
                    <p
                      class={`settings-status ${text.loggedIn === true ? "ok" : "error"}`}
                      role="status"
                    >
                      {text.message}
                      <Show when={text.version}>
                        {(version) => <span class="muted"> ({version()})</span>}
                      </Show>
                    </p>
                  );
                }}
              </Show>
            </div>
          );
        }}
      </For>
    </section>
  );
}
