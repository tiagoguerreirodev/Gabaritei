import { render, screen, waitFor, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AiSettings } from "../bindings/AiSettings";
import type { AiSettingsInput } from "../bindings/AiSettingsInput";
import type { Catalog } from "../bindings/Catalog";
import { resetCatalog } from "../catalog";
import { Toaster, dismissToast } from "../components/toasts";
import Settings from "./Settings";

const role = { agent: "codex", model: "gpt-6.2-sol", effort: "xhigh", fast: false } as const;
const defaults: AiSettings = {
  cliPaths: { claude: null, codex: null },
  roles: { generate: role, blindCheck: role, recheck: role, explain: role },
  remnote: { mode: "off", url: "", tools: [] },
  hasRemnoteToken: false,
};

function backend(
  initial: AiSettings = defaults,
  importing?: Promise<boolean>,
  backingUp?: Promise<unknown>,
  catalog?: Catalog,
) {
  const calls: { command: string; args: unknown }[] = [];
  let stored = initial;
  mockIPC((command, args) => {
    calls.push({ command, args });
    switch (command) {
      case "backup_status":
        return {
          folder: "/dados/backups",
          isDefaultFolder: true,
          backups: [
            {
              name: "backup-20261118-100000-000",
              kind: "backup",
              createdAt: "2026-11-18T10:00:00.000Z",
              sizeBytes: 1024,
            },
          ],
          lastError: null,
          leftBehind: null,
          folderUnavailable: false,
        };
      case "data_folder":
        return "/home/dados/gabaritei";
      case "backup_now":
        return backingUp;
      case "import_data":
        return importing ?? false;
      case "legacy_import_status":
        return { report: null, bankEmpty: false };
      case "ai_catalog":
        if (!catalog) throw new Error("no catalog");
        return catalog;
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

const listed: Catalog = {
  agents: [
    {
      agent: "codex",
      status: { kind: "ready" },
      models: [
        {
          id: "gpt-6.2-sol",
          label: "GPT-6.2-Sol",
          efforts: ["low", "xhigh", "ultra"],
          supportsFast: true,
        },
      ],
    },
    {
      agent: "claude",
      status: { kind: "unavailable", reason: "O Claude Code não informa a lista de modelos." },
      models: [],
    },
  ],
};

beforeEach(() => {
  resetCatalog();
  for (let id = 0; id < 50; id++) dismissToast(id);
});

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

  it("saves the fast flag of a role", async () => {
    const calls = backend();
    const user = userEvent.setup();
    render(() => <Settings />);
    await user.click(await screen.findByRole("checkbox", { name: "Modo rápido: Gerar questões" }));
    await user.click(screen.getByRole("button", { name: "Salvar configurações" }));
    const save = calls.find((c) => c.command === "save_ai_settings");
    const input = (save?.args as { input: AiSettingsInput }).input;
    expect(input.roles.generate.fast).toBe(true);
    expect(input.roles.explain.fast).toBe(false);
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
    await user.selectOptions(
      await screen.findByLabelText("Agente: Explicação sob demanda"),
      "claude",
    );
    // Switching the agent clears the model, so the old agent's name is never kept.
    await user.type(screen.getByLabelText("Modelo: Explicação sob demanda"), "claude-sonnet-5-5");
    await user.click(screen.getByRole("button", { name: "Salvar configurações" }));

    const save = calls.find((c) => c.command === "save_ai_settings");
    const input = (save?.args as { input: AiSettingsInput }).input;
    expect(input.roles.explain).toEqual({
      agent: "claude",
      model: "claude-sonnet-5-5",
      effort: "xhigh",
      fast: false,
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
    // The case the issue names: a restore can't be started behind a running import.
    expect(screen.getByRole("button", { name: /^Restaurar o backup de/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Escolher pasta…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Exportar dados…" })).toBeDisabled();

    // The import was cancelled in the file dialog: everything is available again.
    finish(false);
    await vi.waitFor(() => expect(backupNow).toBeEnabled());
    expect(screen.getByRole("button", { name: "Exportar dados…" })).toBeEnabled();
  });

  it("blocks the data file actions while a backup runs", async () => {
    let finish: (value: unknown) => void = () => undefined;
    backend(
      defaults,
      undefined,
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const user = userEvent.setup();
    render(() => <Settings />);

    await user.click(await screen.findByRole("button", { name: "Fazer backup agora" }));

    expect(screen.getByRole("button", { name: "Importar dados…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Exportar dados…" })).toBeDisabled();
    finish(null);
    await vi.waitFor(() =>
      expect(screen.getByRole("button", { name: "Importar dados…" })).toBeEnabled(),
    );
  });

  it("offers only a direct server connection in RemNote settings", async () => {
    backend(defaults);
    render(() => <Settings />);
    const modes = within(await screen.findByRole("group", { name: "Como conectar" }));
    expect(modes.getAllByRole("radio")).toHaveLength(2);
    expect(modes.getByRole("radio", { name: "Desligado" })).toBeChecked();
    expect(modes.getByRole("radio", { name: "Servidor MCP (endereço e token)" })).toBeEnabled();
  });

  it("feeds the model and effort selectors from the catalog, and reloads them on Atualizar", async () => {
    const calls = backend(defaults, undefined, undefined, listed);
    const user = userEvent.setup();
    render(() => <Settings />);
    const model = await screen.findByRole("combobox", { name: "Modelo: Gerar questões" });
    expect(within(model).getByRole("option", { name: "GPT-6.2-Sol" })).toBeTruthy();
    const effort = screen.getByRole("combobox", { name: "Esforço: Gerar questões" });
    expect(
      within(effort)
        .getAllByRole("option")
        .map((o) => o.textContent),
    ).toEqual(["Baixo", "Extra alto", "Ultra"]);

    await user.click(screen.getByRole("button", { name: "Atualizar listas" }));
    await waitFor(() => {
      const refreshes = calls.filter(
        (c) => c.command === "ai_catalog" && (c.args as { refresh: boolean }).refresh,
      );
      expect(refreshes).toHaveLength(1);
    });
  });

  it("warns about a role whose model the list no longer has, but still lets the settings be saved", async () => {
    const stale: AiSettings = {
      ...defaults,
      roles: {
        ...defaults.roles,
        explain: { agent: "codex", model: "antigo", effort: "low", fast: false },
      },
    };
    const calls = backend(stale, undefined, undefined, listed);
    const user = userEvent.setup();
    render(() => <Settings />);
    expect(await screen.findByText(/“antigo” não está mais disponível/)).toBeTruthy();
    const save = screen.getByRole("button", { name: "Salvar configurações" });
    expect(save).toBeEnabled();
    await user.click(save);
    expect(calls.some((c) => c.command === "save_ai_settings")).toBe(true);
  });

  it("reads the lists again after the CLI path changes", async () => {
    const calls = backend(defaults, undefined, undefined, listed);
    const user = userEvent.setup();
    render(() => <Settings />);
    const codexPath = await screen.findByLabelText("Codex");
    await screen.findByRole("combobox", { name: "Modelo: Gerar questões" });
    const catalogCalls = () => calls.filter((c) => c.command === "ai_catalog").length;
    const before = catalogCalls();

    // Saving without touching the paths leaves the lists alone.
    await user.click(screen.getByRole("button", { name: "Salvar configurações" }));
    await screen.findByText("Configurações salvas.").catch(() => undefined);
    expect(catalogCalls()).toBe(before);

    await user.type(codexPath, "/opt/codex");
    await user.click(screen.getByRole("button", { name: "Salvar configurações" }));
    await waitFor(() => expect(catalogCalls()).toBe(before + 1));
    const last = calls.filter((c) => c.command === "ai_catalog").at(-1);
    expect((last?.args as { refresh: boolean }).refresh).toBe(true);
  });
});
