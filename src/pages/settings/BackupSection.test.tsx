import { render, screen, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { describe, expect, it, vi } from "vitest";
import type { BackupStatus } from "../../bindings/BackupStatus";
import { Toaster } from "../../components/toasts";
import BackupSection from "./BackupSection";

const old = {
  name: "backup-20261118-100000-000",
  kind: "backup",
  createdAt: "2026-11-18T10:00:00.000Z",
  sizeBytes: 3 * 1024 * 1024,
} as const;
const manual = {
  name: "manual-20261119-090000-000",
  kind: "manual",
  createdAt: "2026-11-19T09:00:00.000Z",
  sizeBytes: 2 * 1024 * 1024,
} as const;
const safety = {
  name: "safety-20261119-120000-000",
  kind: "safety",
  createdAt: "2026-11-19T12:00:00.000Z",
  sizeBytes: 1024 * 1024,
} as const;

function backend(initial: Partial<BackupStatus> = {}, fail: Record<string, string> = {}) {
  let status: BackupStatus = {
    folder: "/dados/backups",
    isDefaultFolder: true,
    backups: [safety, manual, old],
    lastError: null,
    ...initial,
  };
  const calls: { command: string; args: unknown }[] = [];
  mockIPC((command, args) => {
    calls.push({ command, args });
    // eslint-disable-next-line @typescript-eslint/only-throw-error -- the backend rejects with a plain { message }
    if (fail[command]) throw { message: fail[command] };
    switch (command) {
      case "backup_status":
        return status;
      case "backup_now": {
        const made = {
          ...old,
          name: "backup-20261120-100000-000",
          createdAt: "2026-11-20T10:00:00.000Z",
        };
        status = { ...status, backups: [made, ...status.backups] };
        return made;
      }
      case "backup_choose_folder":
        status = { ...status, folder: "/outra", isDefaultFolder: false };
        return status;
      case "backup_use_default_folder":
        status = { ...status, folder: "/dados/backups", isDefaultFolder: true };
        return status;
      case "open_folder":
        return null;
      case "backup_restore":
        return null;
      default:
        throw new Error(`Unexpected command: ${command}`);
    }
  });
  return calls;
}

describe("BackupSection", () => {
  it("lists the backups with the safety ones marked", async () => {
    backend();
    render(() => <BackupSection />);

    expect(await screen.findByTestId("backup-folder")).toHaveTextContent("/dados/backups");
    const items = screen.getAllByRole("listitem");
    expect(items).toHaveLength(3);
    const [first, second, third] = items as [HTMLElement, HTMLElement, HTMLElement];
    expect(within(first).getByText("antes de restaurar")).toBeInTheDocument();
    expect(within(second).getByText("manual")).toBeInTheDocument();
    expect(within(third).queryByText("antes de restaurar")).not.toBeInTheDocument();
    expect(within(third).queryByText("manual")).not.toBeInTheDocument();
    expect(within(third).getByText(/3 MB/)).toBeInTheDocument();
  });

  it("opens the backup folder by name, never by path", async () => {
    const calls = backend();
    const user = userEvent.setup();
    render(() => <BackupSection />);

    await user.click(await screen.findByRole("button", { name: "Abrir pasta dos backups" }));

    const open = calls.find((call) => call.command === "open_folder");
    expect(open?.args).toEqual({ which: "backups" });
  });

  it("makes a backup now and shows it", async () => {
    const calls = backend();
    const user = userEvent.setup();
    render(() => (
      <>
        <BackupSection />
        <Toaster />
      </>
    ));

    await user.click(await screen.findByRole("button", { name: "Fazer backup agora" }));

    expect(await screen.findByText("Backup feito.")).toBeInTheDocument();
    expect(calls.map((c) => c.command)).toContain("backup_now");
    expect(await screen.findAllByRole("listitem")).toHaveLength(4);
  });

  it("changes the folder and goes back to the default one", async () => {
    backend();
    const user = userEvent.setup();
    render(() => <BackupSection />);

    await user.click(await screen.findByRole("button", { name: "Escolher pasta…" }));
    expect(await screen.findByTestId("backup-folder")).toHaveTextContent("/outra");

    await user.click(screen.getByRole("button", { name: "Usar a pasta padrão" }));
    expect(await screen.findByText("(padrão)")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Usar a pasta padrão" })).not.toBeInTheDocument();
  });

  it("asks before restoring, then restores and reports it", async () => {
    const calls = backend();
    const onRestored = vi.fn();
    const user = userEvent.setup();
    render(() => (
      <>
        <BackupSection onRestored={onRestored} />
        <Toaster />
      </>
    ));

    await user.click(await screen.findByRole("button", { name: /Restaurar o backup de 18\/11/ }));
    const dialog = await screen.findByRole("dialog");
    expect(calls.map((c) => c.command)).not.toContain("backup_restore");

    await user.click(within(dialog).getByRole("button", { name: "Restaurar" }));

    expect(await screen.findByText("Dados restaurados.")).toBeInTheDocument();
    expect(calls.find((c) => c.command === "backup_restore")?.args).toEqual({ name: old.name });
    expect(onRestored).toHaveBeenCalled();
  });

  it("does not restore when the dialog is cancelled", async () => {
    const calls = backend();
    const user = userEvent.setup();
    render(() => <BackupSection />);

    await user.click(await screen.findByRole("button", { name: /Restaurar o backup de 18\/11/ }));
    await user.click(
      within(await screen.findByRole("dialog")).getByRole("button", { name: "Cancelar" }),
    );

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(calls.map((c) => c.command)).not.toContain("backup_restore");
  });

  it("shows the last backup failure and an empty folder", async () => {
    backend({ backups: [], lastError: "Não foi possível gravar na pasta." });
    render(() => <BackupSection />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "O último backup falhou: Não foi possível gravar na pasta.",
    );
    expect(screen.getByText("Nenhum backup nesta pasta ainda.")).toBeInTheDocument();
  });

  it("keeps the dialog open and shows the error when the restore fails", async () => {
    backend(
      {},
      { backup_restore: "Os dados estão corrompidos ou incompletos. Nada foi alterado." },
    );
    const onRestored = vi.fn();
    const user = userEvent.setup();
    render(() => (
      <>
        <BackupSection onRestored={onRestored} />
        <Toaster />
      </>
    ));

    await user.click(await screen.findByRole("button", { name: /Restaurar o backup de 18\/11/ }));
    await user.click(
      within(await screen.findByRole("dialog")).getByRole("button", { name: "Restaurar" }),
    );

    expect(await screen.findByText(/corrompidos ou incompletos/)).toBeInTheDocument();
    expect(onRestored).not.toHaveBeenCalled();
  });
});
