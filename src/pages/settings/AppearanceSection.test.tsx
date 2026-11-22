import { render, screen, waitFor } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

/** A save the test settles by hand. */
function deferredSaves() {
  const saved: { theme: string; textSize: string }[] = [];
  const pending: { resolve: () => void; reject: () => void }[] = [];
  mockIPC((_command, args) => {
    const value = (args as { appearance: { theme: string; textSize: string } }).appearance;
    saved.push(value);
    return new Promise((resolve, reject) => {
      pending.push({
        resolve: () => resolve(value),
        reject: () => reject(Object.assign(new Error("Falhou."), { message: "Falhou." })),
      });
    });
  });
  return { saved, pending };
}
import { DEFAULT_APPEARANCE, applyAppearance, startAppearance } from "../../appearance";
import { Toaster } from "../../components/toasts";
import AppearanceSection from "./AppearanceSection";

const root = document.documentElement;

describe("Aparência", () => {
  beforeEach(() => {
    localStorage.clear();
    applyAppearance(DEFAULT_APPEARANCE);
  });

  afterEach(() => {
    root.removeAttribute("data-theme");
    root.removeAttribute("data-text-size");
  });

  it("applies and saves the theme and the text size as soon as they are chosen", async () => {
    const saved: unknown[] = [];
    mockIPC((command, args) => {
      if (command !== "save_appearance") throw new Error(`Unexpected command: ${command}`);
      saved.push((args as { appearance: unknown }).appearance);
      return (args as { appearance: unknown }).appearance;
    });
    const user = userEvent.setup();
    render(() => <AppearanceSection />);

    expect(screen.getByRole("radio", { name: /Automático/ })).toBeChecked();
    expect(screen.getByRole("radio", { name: "Médio" })).toBeChecked();

    await user.click(screen.getByRole("radio", { name: "Escuro" }));
    await waitFor(() => expect(root.getAttribute("data-theme")).toBe("dark"));
    await user.click(screen.getByRole("radio", { name: "Muito grande" }));
    await waitFor(() => expect(root.getAttribute("data-text-size")).toBe("xl"));

    expect(saved).toEqual([
      { theme: "dark", textSize: "m" },
      { theme: "dark", textSize: "xl" },
    ]);
    expect(screen.getByRole("radio", { name: "Escuro" })).toBeChecked();
    expect(JSON.parse(localStorage.getItem("gabaritei.appearance") ?? "null")).toEqual({
      theme: "dark",
      textSize: "xl",
    });
  });

  it("keeps keyboard focus on the newly selected radio while saving", async () => {
    mockIPC((_command, args) => (args as { appearance: unknown }).appearance);
    const user = userEvent.setup();
    render(() => <AppearanceSection />);

    screen.getByRole("radio", { name: /Automático/ }).focus();
    await user.keyboard("{ArrowDown}");
    const claro = screen.getByRole("radio", { name: "Claro" });
    expect(claro).toHaveFocus();
    expect(claro).toBeEnabled();
    await waitFor(() => expect(root.getAttribute("data-theme")).toBe("light"));
    expect(claro).toHaveFocus();
  });

  it("returns to following the system by removing the theme attribute", async () => {
    mockIPC((_command, args) => (args as { appearance: unknown }).appearance);
    const user = userEvent.setup();
    render(() => <AppearanceSection />);

    await user.click(screen.getByRole("radio", { name: "Claro" }));
    await waitFor(() => expect(root.getAttribute("data-theme")).toBe("light"));
    await user.click(screen.getByRole("radio", { name: /Automático/ }));
    await waitFor(() => expect(root).not.toHaveAttribute("data-theme"));
  });

  it("keeps the current choice and tells the user when saving fails", async () => {
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
    mockIPC(() => Promise.reject({ message: "Não foi possível salvar." }));
    const user = userEvent.setup();
    render(() => (
      <>
        <AppearanceSection />
        <Toaster />
      </>
    ));

    await user.click(screen.getByRole("radio", { name: "Escuro" }));
    expect(await screen.findByText("Não foi possível salvar.")).toBeInTheDocument();
    expect(root).not.toHaveAttribute("data-theme");
    expect(screen.getByRole("radio", { name: /Automático/ })).toBeChecked();
  });
});

describe("Aparência com salvamento pendente", () => {
  beforeEach(() => {
    localStorage.clear();
    applyAppearance(DEFAULT_APPEARANCE);
  });
  afterEach(() => {
    root.removeAttribute("data-theme");
    root.removeAttribute("data-text-size");
  });

  it("saves both a theme and a text size chosen while the first save is pending", async () => {
    const { saved, pending } = deferredSaves();
    const user = userEvent.setup();
    render(() => <AppearanceSection />);

    await user.click(screen.getByRole("radio", { name: "Escuro" }));
    await user.click(screen.getByRole("radio", { name: "Muito grande" }));
    expect(screen.getByRole("radio", { name: "Escuro" })).toBeChecked();
    expect(screen.getByRole("radio", { name: "Muito grande" })).toBeChecked();

    pending[0]?.resolve();
    await waitFor(() => expect(saved).toHaveLength(2));
    expect(saved[1]).toEqual({ theme: "dark", textSize: "xl" });
    pending[1]?.resolve();
    await waitFor(() => expect(root.getAttribute("data-text-size")).toBe("xl"));
    expect(root.getAttribute("data-theme")).toBe("dark");
  });

  it("saves only the latest of two choices in the same group", async () => {
    const { saved, pending } = deferredSaves();
    const user = userEvent.setup();
    render(() => <AppearanceSection />);

    await user.click(screen.getByRole("radio", { name: "Escuro" }));
    await user.click(screen.getByRole("radio", { name: "Claro" }));
    await user.click(screen.getByRole("radio", { name: /Automático/ }));
    await user.click(screen.getByRole("radio", { name: "Claro" }));
    pending[0]?.resolve();
    await waitFor(() => expect(saved).toHaveLength(2));
    expect(saved[1]).toEqual({ theme: "light", textSize: "m" });
    pending[1]?.resolve();
    await waitFor(() => expect(root.getAttribute("data-theme")).toBe("light"));
    expect(saved).toHaveLength(2);
  });

  it("on a failure shows a toast and still saves the choice queued meanwhile", async () => {
    const { saved, pending } = deferredSaves();
    const user = userEvent.setup();
    render(() => (
      <>
        <AppearanceSection />
        <Toaster />
      </>
    ));

    await user.click(screen.getByRole("radio", { name: "Escuro" }));
    await user.click(screen.getByRole("radio", { name: "Muito grande" }));
    pending[0]?.reject();
    expect(await screen.findByText("Falhou.")).toBeInTheDocument();
    // The queued value is saved as the user last intended it (theme included).
    await waitFor(() => expect(saved).toHaveLength(2));
    expect(saved[1]).toEqual({ theme: "dark", textSize: "xl" });
    pending[1]?.resolve();
    await waitFor(() => expect(root.getAttribute("data-theme")).toBe("dark"));
  });

  it("snaps the radios back when the only save fails", async () => {
    const { pending } = deferredSaves();
    const user = userEvent.setup();
    render(() => (
      <>
        <AppearanceSection />
        <Toaster />
      </>
    ));

    await user.click(screen.getByRole("radio", { name: "Escuro" }));
    expect(screen.getByRole("radio", { name: "Escuro" })).toBeChecked();
    pending[0]?.reject();
    expect(await screen.findByText("Falhou.")).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("radio", { name: /Automático/ })).toBeChecked());
  });
});

describe("startAppearance", () => {
  beforeEach(() => localStorage.clear());

  afterEach(() => {
    root.removeAttribute("data-theme");
    root.removeAttribute("data-text-size");
  });

  it("applies the cached choice first and then the saved one", async () => {
    localStorage.setItem("gabaritei.appearance", JSON.stringify({ theme: "dark", textSize: "l" }));
    mockIPC(() => ({ theme: "light", textSize: "s" }));

    const done = startAppearance();
    expect(root.getAttribute("data-theme")).toBe("dark");
    expect(root.getAttribute("data-text-size")).toBe("l");

    await done;
    expect(root.getAttribute("data-theme")).toBe("light");
    expect(root.getAttribute("data-text-size")).toBe("s");
  });

  it("ignores a corrupt cache and keeps the defaults when the backend is unavailable", async () => {
    localStorage.setItem("gabaritei.appearance", "{not json");
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
    mockIPC(() => Promise.reject("offline"));

    await startAppearance();
    expect(root).not.toHaveAttribute("data-theme");
    expect(root.getAttribute("data-text-size")).toBe("m");
  });
});
