import { render, screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { createSignal, Show } from "solid-js";
import { afterEach, describe, expect, it, vi } from "vitest";
import Modal from "./Modal";
import { TOAST_DURATION, Toaster, showError, showToast } from "./toasts";

describe("Modal", () => {
  function Harness() {
    const [open, setOpen] = createSignal(false);
    return (
      <>
        <button type="button" onClick={() => setOpen(true)}>
          Abrir
        </button>
        <Show when={open()}>
          <Modal title="Detalhes" onClose={() => setOpen(false)}>
            <p>Conteúdo</p>
          </Modal>
        </Show>
      </>
    );
  }

  it("is a labelled dialog that takes focus and gives it back when closed", async () => {
    const user = userEvent.setup();
    render(() => <Harness />);
    const opener = screen.getByRole("button", { name: "Abrir" });

    await user.click(opener);
    const dialog = screen.getByRole("dialog", { name: "Detalhes" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(dialog).toHaveFocus();

    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(opener).toHaveFocus();
  });

  it("closes with the close button and with a click on the backdrop, not inside", async () => {
    const user = userEvent.setup();
    render(() => <Harness />);

    await user.click(screen.getByRole("button", { name: "Abrir" }));
    await user.click(screen.getByRole("button", { name: "Fechar" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Abrir" }));
    await user.click(screen.getByText("Conteúdo"));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    await user.click(document.querySelector(".modal-backdrop")!);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});

describe("toasts", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("shows messages, errors as alerts, and hides them after a while", () => {
    vi.useFakeTimers();
    render(() => <Toaster />);

    showToast("Banco importado.");
    showError(new Error("Não foi possível ler o banco."));
    expect(screen.getByRole("status")).toHaveTextContent("Banco importado.");
    expect(screen.getByRole("alert")).toHaveTextContent("Não foi possível ler o banco.");

    vi.advanceTimersByTime(TOAST_DURATION);
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("can be dismissed", async () => {
    const user = userEvent.setup();
    render(() => <Toaster />);

    showToast("Aviso.");
    await user.click(screen.getByRole("button", { name: "Dispensar aviso" }));
    expect(screen.queryByText("Aviso.")).not.toBeInTheDocument();
  });
});
