import { render, screen, waitFor } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
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
