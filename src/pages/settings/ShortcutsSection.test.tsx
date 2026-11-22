import { render, screen, waitFor } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { describe, expect, it } from "vitest";
import { shortcutsEnabled } from "../../shortcuts";
import { Toaster } from "../../components/toasts";
import ShortcutsSection from "./ShortcutsSection";

describe("Atalhos de teclado", () => {
  it("loads the saved value, then saves a change at once", async () => {
    const saved: boolean[] = [];
    mockIPC((command, args) => {
      if (command === "keyboard_shortcuts") return true;
      if (command === "save_keyboard_shortcuts") {
        saved.push((args as { enabled: boolean }).enabled);
        return (args as { enabled: boolean }).enabled;
      }
      throw new Error(`Unexpected command: ${command}`);
    });
    const user = userEvent.setup();
    render(() => <ShortcutsSection />);

    const box = screen.getByRole("checkbox", { name: "Atalhos de teclado" });
    await waitFor(() => expect(box).toBeChecked());
    await user.click(box);
    await waitFor(() => expect(saved).toEqual([false]));
    expect(box).not.toBeChecked();
    expect(shortcutsEnabled()).toBe(false);

    await user.click(box);
    await waitFor(() => expect(saved).toEqual([false, true]));
    expect(shortcutsEnabled()).toBe(true);
  });

  it("goes back to the saved value when saving fails", async () => {
    mockIPC((command) => {
      if (command === "keyboard_shortcuts") return true;
      // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
      return Promise.reject({ message: "Não foi possível salvar." });
    });
    const user = userEvent.setup();
    render(() => (
      <>
        <ShortcutsSection />
        <Toaster />
      </>
    ));

    const box = screen.getByRole("checkbox", { name: "Atalhos de teclado" });
    await user.click(box);
    await screen.findByText("Não foi possível salvar.");
    await waitFor(() => expect(box).toBeChecked());
    expect(shortcutsEnabled()).toBe(true);
  });
});
