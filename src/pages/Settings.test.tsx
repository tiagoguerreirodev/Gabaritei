import { render, screen, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { describe, expect, it, vi } from "vitest";
import type { AiSettings } from "../bindings/AiSettings";
import type { AiSettingsInput } from "../bindings/AiSettingsInput";
import { Toaster } from "../components/toasts";
import Settings from "./Settings";

const role = { agent: "codex", model: "gpt-6.2-sol", effort: "xhigh" } as const;
const defaults: AiSettings = {
  cliPaths: { claude: null, codex: null },
  roles: { generate: role, blindCheck: role, recheck: role, explain: role },
  remnote: { mode: "off", url: "", tools: [] },
  hasRemnoteToken: false,
};

function backend(initial: AiSettings = defaults, importing?: Promise<boolean>) {
  const calls: { command: string; args: unknown }[] = [];
  let stored = initial;
  mockIPC((command, args) => {
    calls.push({ command, args });
    switch (command) {
      case "backup_status":
        return {
          folder: "/dados/backups",
          isDefaultFolder: true,
          backups: [],
          lastError: null,
        };
      case "import_data":
        return importing ?? false;
      case "legacy_import_status":
        return { report: null, bankEmpty: false };
      case "ai_settings":
        return stored;
      case "save_ai_settings": {
        const { input } = args as { input: AiSettingsInput };
        stored = { ...input, hasRemnoteToken: stored.hasRemnoteToken };
        return stored;
      }
      case "set_remnote_token": {
        const token = (args as { token: string | null }).token;
        stored = { ...stored, hasRemnoteToken: Boolean(token) };
        return stored.hasRemnoteToken;
      }
      case "cli_test":
        return {
          agent: "codex",
          path: "/usr/bin/codex",
          version: "codex-cli 0.171.0",
          loggedIn: true,
          message: "Codex: pronto para uso.",
        };
      case "remnote_test":
        return {
          ok: true,
          message: "Conexão com o RemNote funcionando.",
          tools: ["create_note", "search"],
        };
      default:
        throw new Error(`Unexpected command: ${command}`);
    }
  });
  return calls;
}

describe("Configurações", () => {
  it("shows the four roles with the defaults", async () => {
    backend();
    render(() => <Settings />);
    const table = await screen.findByRole("table");
    for (const name of [
      "Gerar questões",
      "Conferência às cegas",
      "Revisão sob demanda",
      "Explicação sob demanda",
    ]) {
      const row = within(table).getByRole("row", { name: new RegExp(name) });
      expect(within(row).getByDisplayValue("gpt-6.2-sol")).toBeTruthy();
      expect(within(row).getByDisplayValue("Extra alto")).toBeTruthy();
      expect(within(row).getByDisplayValue("Codex")).toBeTruthy();
    }
  });

  it("saves an edited role", async () => {
    const calls = backend();
    const user = userEvent.setup();
    render(() => (
      <>
        <Settings />
        <Toaster />
      </>
    ));
    const model = await screen.findByLabelText("Modelo: Explicação sob demanda");
    await user.clear(model);
    await user.type(model, "claude-sonnet-5-5");
    await user.selectOptions(screen.getByLabelText("Agente: Explicação sob demanda"), "claude");
    await user.click(screen.getByRole("button", { name: "Salvar configurações" }));

    const save = calls.find((c) => c.command === "save_ai_settings");
    const input = (save?.args as { input: AiSettingsInput }).input;
    expect(input.roles.explain).toEqual({
      agent: "claude",
      model: "claude-sonnet-5-5",
      effort: "xhigh",
    });
    expect(input.roles.generate.model).toBe("gpt-6.2-sol");
    expect(await screen.findByText("Configurações salvas.")).toBeTruthy();
  });

  it("tests a CLI and shows its version and login state", async () => {
    backend();
    const user = userEvent.setup();
    render(() => <Settings />);
    await screen.findByRole("table");
    const [first] = screen.getAllByRole("button", { name: "Testar" });
    if (!first) throw new Error("no Testar button");
    await user.click(first);
    expect(await screen.findByText(/Codex: pronto para uso\./)).toBeTruthy();
    expect(screen.getByText(/codex-cli 0\.171\.0/)).toBeTruthy();
  });

  it("keeps the RemNote token out of the saved settings and the screen", async () => {
    const calls = backend({
      ...defaults,
      remnote: { mode: "server", url: "https://r.example/mcp", tools: [] },
    });
    const user = userEvent.setup();
    render(() => <Settings />);
    const field = await screen.findByLabelText("Token");
    expect(field.getAttribute("type")).toBe("password");
    await user.type(field, "TOKEN-SECRETO");
    await user.click(screen.getByRole("button", { name: "Guardar token" }));

    expect((await screen.findByLabelText("Token")).getAttribute("placeholder")).toMatch(
      /Token guardado/,
    );
    expect(screen.getByLabelText<HTMLInputElement>("Token").value).toBe("");
    await user.click(screen.getByRole("button", { name: "Salvar configurações" }));
    const save = calls.find((c) => c.command === "save_ai_settings");
    expect(JSON.stringify(save?.args)).not.toContain("TOKEN-SECRETO");
    expect(document.body.textContent).not.toContain("TOKEN-SECRETO");
  });

  it("lets the user approve RemNote tools found by the test", async () => {
    const calls = backend({
      ...defaults,
      remnote: { mode: "server", url: "https://r.example/mcp", tools: [] },
      hasRemnoteToken: true,
    });
    const user = userEvent.setup();
    render(() => <Settings />);
    await user.click(await screen.findByRole("button", { name: "Testar conexão" }));
    expect(await screen.findByText("Conexão com o RemNote funcionando.")).toBeTruthy();
    await user.click(screen.getByRole("checkbox", { name: "search" }));
    await user.click(screen.getByRole("button", { name: "Salvar configurações" }));

    const saves = calls.filter((c) => c.command === "save_ai_settings");
    const last = (saves.at(-1)?.args as { input: AiSettingsInput }).input;
    expect(last.remnote.tools).toEqual(["search"]);
  });

  it("blocks backup actions while an import runs, because Dados has one busy state", async () => {
    let finish: (imported: boolean) => void = () => undefined;
    backend(
      defaults,
      new Promise<boolean>((resolve) => {
        finish = resolve;
      }),
    );
    const user = userEvent.setup();
    render(() => <Settings />);
    const backupNow = await screen.findByRole("button", { name: "Fazer backup agora" });
    expect(backupNow).toBeEnabled();

    await user.click(screen.getByRole("button", { name: "Importar dados…" }));
    await user.click(screen.getByRole("button", { name: "Escolher arquivo e importar" }));

    expect(backupNow).toBeDisabled();
    expect(screen.getByRole("button", { name: "Escolher pasta…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Exportar dados…" })).toBeDisabled();

    // The import was cancelled in the file dialog: everything is available again.
    finish(false);
    await vi.waitFor(() => expect(backupNow).toBeEnabled());
    expect(screen.getByRole("button", { name: "Exportar dados…" })).toBeEnabled();
  });

  it("warns that the Codex config mode cannot limit RemNote tools", async () => {
    backend({ ...defaults, remnote: { mode: "codexConfig", url: "", tools: [] } });
    render(() => <Settings />);
    expect(await screen.findByRole("note")).toBeTruthy();
    expect(screen.getByRole("note").textContent).toMatch(/não consegue limitar/);
  });
});
