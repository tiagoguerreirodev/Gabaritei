import { render, screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { createSignal } from "solid-js";
import { describe, expect, it, vi } from "vitest";
import MasteryBar from "./MasteryBar";
import { EmptyState, ErrorState, Loading } from "./states";

describe("Loading", () => {
  it("keeps the text variant as a status message", () => {
    render(() => <Loading label="Carregando cadernos…" />);
    expect(screen.getByRole("status")).toHaveTextContent("Carregando cadernos…");
  });

  it("announces the label once in the skeleton variant and hides the placeholder lines", () => {
    const { container } = render(() => <Loading variant="skeleton" label="Calculando…" />);
    const status = screen.getByRole("status");
    expect(status).toHaveTextContent("Calculando…");
    expect(status).not.toHaveAttribute("aria-busy");
    expect(container.querySelectorAll(".skeleton-line[aria-hidden='true']")).toHaveLength(3);
  });
});

describe("EmptyState", () => {
  it("renders the action slot after the explanation", async () => {
    const onClick = vi.fn();
    render(() => (
      <EmptyState
        title="Nada por aqui."
        action={
          <button type="button" onClick={onClick}>
            Começar
          </button>
        }
      >
        <p>Explicação.</p>
      </EmptyState>
    ));
    expect(screen.getByText("Nada por aqui.")).toBeVisible();
    await userEvent.setup().click(screen.getByRole("button", { name: "Começar" }));
    expect(onClick).toHaveBeenCalledOnce();
  });

  it("renders no action wrapper without an action", () => {
    const { container } = render(() => <EmptyState title="Vazio." />);
    expect(container.querySelector(".state-action")).toBeNull();
  });
});

describe("ErrorState", () => {
  it("shows the message and calls back on Tentar novamente", async () => {
    const onRetry = vi.fn();
    render(() => <ErrorState error={new Error("Falhou ao carregar.")} onRetry={onRetry} />);
    expect(screen.getByRole("alert")).toHaveTextContent("Falhou ao carregar.");
    await userEvent.setup().click(screen.getByRole("button", { name: "Tentar novamente" }));
    expect(onRetry).toHaveBeenCalledOnce();
  });

  it("disables the button while busy and announces each new error again", () => {
    const [error, setError] = createSignal<unknown>(new Error("Falhou."));
    const [busy, setBusy] = createSignal(false);
    render(() => <ErrorState error={error()} onRetry={() => undefined} busy={busy()} />);
    const first = screen.getByRole("alert");
    setBusy(true);
    expect(screen.getByRole("button", { name: "Tentando…" })).toBeDisabled();
    setBusy(false);
    expect(screen.getByRole("alert")).toBe(first);
    setError(new Error("Falhou."));
    expect(screen.getByRole("alert")).not.toBe(first);
    expect(screen.getByRole("button", { name: "Tentar novamente" })).toBeEnabled();
  });

  it("has no retry button without a callback", () => {
    render(() => <ErrorState error="Erro." />);
    expect(screen.queryByRole("button")).toBeNull();
  });
});

describe("MasteryBar", () => {
  it("exposes the backend achievement as a meter with a text value", () => {
    render(() => <MasteryBar name="Direito" achievement={0.7} />);
    const meter = screen.getByRole("meter", { name: "Aproveitamento em Direito" });
    expect(meter).toHaveAttribute("aria-valuenow", "70");
    expect(meter).toHaveAttribute("aria-valuemin", "0");
    expect(meter).toHaveAttribute("aria-valuemax", "100");
    expect(meter).toHaveAttribute("aria-valuetext", "70% de aproveitamento");
    expect(screen.getByText("70%")).toBeVisible();
  });

  it("hides the visible name and number from assistive tech, the meter carries both", () => {
    render(() => <MasteryBar name="Direito" achievement={0.7} />);
    expect(screen.getByText("Direito")).toHaveAttribute("aria-hidden", "true");
    expect(screen.getByText("70%")).toHaveAttribute("aria-hidden", "true");
  });

  it("says so when nothing was evaluated", () => {
    render(() => <MasteryBar name="Informática" achievement={null} />);
    const meter = screen.getByRole("meter");
    expect(meter).toHaveAttribute("aria-valuenow", "0");
    expect(meter).toHaveAttribute("aria-valuetext", "Sem questões avaliadas");
    expect(screen.getByText("—")).toBeVisible();
  });
});
