import { For, Show, createSignal } from "solid-js";
import { api } from "../../api";
import type { RemNoteMode } from "../../bindings/RemNoteMode";
import type { RemNoteSettings } from "../../bindings/RemNoteSettings";
import type { RemNoteTest } from "../../bindings/RemNoteTest";
import { errorMessage } from "../../components/states";
import { showError, showSuccess } from "../../components/toasts";

const modes: { value: RemNoteMode; label: string }[] = [
  { value: "off", label: "Desligado" },
  { value: "server", label: "Servidor MCP (endereço e token)" },
];

/** RemNote as a source: server URL, token (kept in the OS keychain) and the approved tools. */
export default function RemNoteSection(props: {
  remnote: RemNoteSettings;
  hasToken: boolean;
  onChange: (remnote: RemNoteSettings) => void;
  onTokenSaved: (hasToken: boolean) => void;
  /** Saves the settings first, so the test uses what is on screen. */
  beforeTest: () => Promise<boolean>;
}) {
  const [token, setToken] = createSignal("");
  const [result, setResult] = createSignal<RemNoteTest | null>(null);
  const [testing, setTesting] = createSignal(false);

  const saveToken = async (value: string | null) => {
    try {
      props.onTokenSaved(await api.setRemnoteToken(value));
      setToken("");
      showSuccess(value ? "Token guardado no chaveiro do sistema." : "Token removido.");
    } catch (error) {
      showError(error);
    }
  };

  const test = async () => {
    setTesting(true);
    try {
      if (!(await props.beforeTest())) return;
      setResult(await api.remnoteTest());
    } catch (error) {
      setResult({ ok: false, message: errorMessage(error), tools: [] });
    } finally {
      setTesting(false);
    }
  };

  const toggleTool = (tool: string, on: boolean) => {
    const tools = on
      ? [...props.remnote.tools, tool]
      : props.remnote.tools.filter((name) => name !== tool);
    props.onChange({ ...props.remnote, tools });
  };

  /** Approved tools plus whatever the last test found. */
  const toolNames = () => [...new Set([...props.remnote.tools, ...(result()?.tools ?? [])])].sort();

  return (
    <section class="card" aria-labelledby="remnote-title">
      <h2 id="remnote-title">RemNote</h2>
      <p class="muted">
        Deixa a IA consultar suas anotações do RemNote ao gerar questões. Só ferramentas de leitura
        são permitidas.
      </p>
      <fieldset class="choice-group">
        <legend>Como conectar</legend>
        <For each={modes}>
          {(mode) => (
            <label>
              <input
                type="radio"
                name="remnote-mode"
                value={mode.value}
                checked={props.remnote.mode === mode.value}
                onChange={() => props.onChange({ ...props.remnote, mode: mode.value })}
              />{" "}
              {mode.label}
            </label>
          )}
        </For>
      </fieldset>

      <Show when={props.remnote.mode === "server"}>
        <div class="settings-row">
          <label for="remnote-url">Endereço do servidor</label>
          <input
            class="input"
            id="remnote-url"
            type="url"
            placeholder="https://…"
            value={props.remnote.url}
            onInput={(event) =>
              props.onChange({ ...props.remnote, url: event.currentTarget.value })
            }
          />
        </div>
        <div class="settings-row">
          <label for="remnote-token">Token</label>
          <input
            class="input"
            id="remnote-token"
            type="password"
            autocomplete="off"
            placeholder={props.hasToken ? "Token guardado (digite para trocar)" : "Cole o token"}
            value={token()}
            onInput={(event) => setToken(event.currentTarget.value)}
          />
          <button
            type="button"
            class="btn"
            disabled={token().trim() === ""}
            onClick={() => void saveToken(token())}
          >
            Guardar token
          </button>
          <Show when={props.hasToken}>
            <button type="button" class="btn" onClick={() => void saveToken(null)}>
              Remover token
            </button>
          </Show>
        </div>
        <p class="muted">O token fica no chaveiro do sistema, nunca no banco de dados.</p>
      </Show>

      <Show when={props.remnote.mode === "server"}>
        <button type="button" class="btn" disabled={testing()} onClick={() => void test()}>
          {testing() ? "Testando…" : "Testar conexão"}
        </button>
        <Show when={result()}>
          {(current) => (
            <p class={`settings-status ${current().ok ? "ok" : "error"}`} role="status">
              {current().message}
            </p>
          )}
        </Show>
      </Show>

      <Show when={props.remnote.mode === "server" && toolNames().length > 0}>
        <fieldset class="choice-group">
          <legend>Ferramentas permitidas (somente leitura)</legend>
          <For each={toolNames()}>
            {(tool) => (
              <label>
                <input
                  type="checkbox"
                  checked={props.remnote.tools.includes(tool)}
                  onChange={(event) => toggleTool(tool, event.currentTarget.checked)}
                />{" "}
                <code>{tool}</code>
              </label>
            )}
          </For>
        </fieldset>
      </Show>
    </section>
  );
}
