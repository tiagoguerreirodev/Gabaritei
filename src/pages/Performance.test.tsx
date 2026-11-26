import { HashRouter, Route } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it } from "vitest";
import type { Performance as PerformanceData } from "../bindings/Performance";
import type { PerformanceQuery } from "../bindings/PerformanceQuery";
import { Toaster, dismissToast } from "../components/toasts";
import { emptyPerformance, stats } from "../test/fixtures";
import Performance from "./Performance";

const direito = stats(8, 2, 0, 20);
const crase = stats(1, 2, 0, 5);
const data: PerformanceData = {
  overall: stats(9, 4, 0, 25),
  areas: [
    {
      name: "Português",
      stats: crase,
      matters: [
        {
          name: "Gramática",
          stats: crase,
          topics: [{ topicId: 3, name: "Crase", stats: crase, trend: [] }],
        },
      ],
    },
    {
      name: "Direito",
      stats: direito,
      matters: [
        {
          name: "Constitucional",
          stats: direito,
          topics: [
            {
              topicId: 1,
              name: "Controle",
              stats: direito,
              trend: [
                { weekStart: "2026-11-09", answered: 5, correct: 3, accuracy: 0.6 },
                { weekStart: "2026-11-16", answered: 5, correct: 4, accuracy: 0.8 },
              ],
            },
          ],
        },
      ],
    },
  ],
  weakest: [
    { topicId: 3, area: "Português", matter: "Gramática", topic: "Crase", stats: crase },
    { topicId: 1, area: "Direito", matter: "Constitucional", topic: "Controle", stats: direito },
  ],
  trend: [
    { weekStart: "2026-11-09", answered: 5, correct: 3, accuracy: 0.6 },
    { weekStart: "2026-11-23", answered: 8, correct: 6, accuracy: 0.75 },
  ],
  filters: { contests: ["INSS", "TRT"], boards: ["Cebraspe", "FGV"] },
};

function backend(result: PerformanceData | { error: string }, exported = true) {
  const failures = { next: false };
  const queries: PerformanceQuery[] = [];
  const exports: PerformanceQuery[] = [];
  const started: unknown[] = [];
  mockIPC((command, args) => {
    if (command === "question_count_limits") return { min: 1, max: 20, initial: 10 };
    if (command === "session_start") {
      started.push(args);
      return new Promise(() => {});
    }
    const query = (args as { query: PerformanceQuery }).query;
    if (command === "performance") {
      queries.push(query);
      if (failures.next || "error" in result) {
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        return Promise.reject({
          message: "error" in result ? result.error : "Falha ao calcular.",
        });
      }
      return result;
    }
    if (command === "export_performance_csv") {
      exports.push(query);
      return exported;
    }
    throw new Error(`Unexpected command: ${command}`);
  });
  return { queries, exports, failures, started };
}

/** Toasts live in module state; forget the ones earlier tests raised. */
afterEach(() => {
  for (let id = 0; id < 100; id++) dismissToast(id);
});

describe("Desempenho", () => {
  it("shows the summary cards, the weakest topics first, the trend and the table", async () => {
    backend(data);
    render(() => <Performance />);

    const cards = await screen.findByRole("list", { name: "Resumo" });
    expect(within(cards).getByText("Aproveitamento").nextElementSibling).toHaveTextContent("69,2%");
    expect(
      within(cards).getByText("Acerto entre respondidas").nextElementSibling,
    ).toHaveTextContent("69,2%");
    expect(within(cards).getByText("Cobertura").nextElementSibling).toHaveTextContent("52%");

    const weakest = screen.getByRole("heading", { name: "Assuntos mais fracos" }).parentElement!;
    const items = within(weakest).getAllByRole("listitem");
    expect(items[0]).toHaveTextContent("Crase");
    expect(items[0]).toHaveTextContent("33,3%");
    expect(items[0]).toHaveTextContent("amostra pequena");
    expect(items[1]).toHaveTextContent("Controle");
    expect(items[1]).not.toHaveTextContent("amostra pequena");

    const chart = screen.getByRole("img", { name: "Acerto semanal" });
    expect(chart.querySelectorAll("circle")).toHaveLength(2);
    expect(chart).toHaveTextContent("Semana de 09/11: 60% (3 de 5 respostas)");
    expect(chart).toHaveTextContent("Semana de 23/11: 75% (6 de 8 respostas)");
  });

  it("expands areas and matters in the table, weakest first", async () => {
    backend(data);
    const user = userEvent.setup();
    render(() => <Performance />);

    const table = await screen.findByRole("table", { name: /Desempenho por área/ });
    const rowNames = () =>
      within(table)
        .getAllByRole("row")
        .slice(1)
        .map((row) => within(row).getAllByRole("rowheader")[0]?.textContent?.trim());
    expect(rowNames()).toEqual(["Português amostra pequena", "Direito"]);

    await user.click(within(table).getByRole("button", { name: "Direito" }));
    await user.click(within(table).getByRole("button", { name: "Constitucional" }));
    expect(rowNames()).toEqual([
      "Português amostra pequena",
      "Direito",
      "Constitucional",
      "Controle",
    ]);
    const topicRow = within(table).getByRole("row", { name: /Controle/ });
    expect(topicRow).toHaveTextContent("80%");
  });

  it("sorts the table alphabetically on request", async () => {
    backend(data);
    const user = userEvent.setup();
    render(() => <Performance />);
    const table = await screen.findByRole("table", { name: /Desempenho por área/ });
    const areaNames = () =>
      within(table)
        .getAllByRole("button", { expanded: false })
        .map((button) => button.textContent);
    expect(areaNames()).toEqual(["Português", "Direito"]);

    await user.click(screen.getByRole("radio", { name: "Ordem alfabética" }));
    expect(areaNames()).toEqual(["Direito", "Português"]);

    await user.click(screen.getByRole("radio", { name: "Mais fraco primeiro" }));
    expect(areaNames()).toEqual(["Português", "Direito"]);
  });

  it("shows a mini trend for each topic and trains it from the table", async () => {
    const { started } = backend(data);
    const user = userEvent.setup();
    render(() => <Performance />);
    const table = await screen.findByRole("table", { name: /Desempenho por área/ });
    await user.click(within(table).getByRole("button", { name: "Direito" }));
    await user.click(within(table).getByRole("button", { name: "Constitucional" }));

    expect(
      within(table).getByRole("img", {
        name: "Acerto de 60% a 80% nas últimas 2 semanas com respostas",
      }),
    ).toBeInTheDocument();

    await user.click(within(table).getByRole("button", { name: "Treinar este assunto: Controle" }));
    const dialog = await screen.findByRole("dialog", {
      name: "Direito › Constitucional › Controle",
    });
    expect(await within(dialog).findByLabelText(/Quantidade de questões/)).toHaveValue(10);
    await user.click(within(dialog).getByRole("button", { name: "Começar" }));
    expect(started).toEqual([
      {
        request: {
          source: { kind: "topic", topicId: 1, count: 10 },
          feedbackMode: "study",
        },
      },
    ]);
  });

  it("sends the filters and the time zone offset to the backend", async () => {
    const { queries } = backend(data);
    const user = userEvent.setup();
    render(() => <Performance />);
    await screen.findByRole("list", { name: "Resumo" });
    expect(queries[0]).toEqual({
      contest: null,
      board: null,
      feedbackMode: null,
      utcOffsetMinutes: -new Date().getTimezoneOffset(),
    });

    await user.selectOptions(screen.getByLabelText("Concurso"), "TRT");
    await user.selectOptions(screen.getByLabelText("Banca"), "FGV");
    await user.selectOptions(screen.getByLabelText("Modo"), "Simulado");

    await waitFor(() => expect(queries).toHaveLength(4));
    expect(queries.at(-1)).toMatchObject({
      contest: "TRT",
      board: "FGV",
      feedbackMode: "exam",
    });
  });

  it("exports the CSV with the active filters and confirms with a toast", async () => {
    const { exports } = backend(data, true);
    const user = userEvent.setup();
    render(() => (
      <>
        <Performance />
        <Toaster />
      </>
    ));
    await screen.findByRole("list", { name: "Resumo" });

    await user.selectOptions(screen.getByLabelText("Concurso"), "INSS");
    await user.click(screen.getByRole("button", { name: "Exportar CSV" }));

    expect(await screen.findByText("Planilha salva.")).toBeVisible();
    expect(exports).toHaveLength(1);
    expect(exports[0]).toMatchObject({ contest: "INSS" });
  });

  it("says nothing when the save dialog is cancelled", async () => {
    const { exports } = backend(data, false);
    const user = userEvent.setup();
    render(() => (
      <>
        <Performance />
        <Toaster />
      </>
    ));
    await screen.findByRole("list", { name: "Resumo" });

    await user.click(screen.getByRole("button", { name: "Exportar CSV" }));

    await waitFor(() => expect(exports).toHaveLength(1));
    await waitFor(() => expect(screen.getByRole("button", { name: "Exportar CSV" })).toBeEnabled());
    expect(screen.queryByText("Planilha salva.")).not.toBeInTheDocument();
  });

  it("keeps the tag options when a reload fails", async () => {
    const { failures } = backend(data);
    const user = userEvent.setup();
    render(() => <Performance />);
    await screen.findByRole("list", { name: "Resumo" });

    failures.next = true;
    await user.selectOptions(screen.getByLabelText("Concurso"), "TRT");

    expect(await screen.findByRole("alert")).toHaveTextContent("Falha ao calcular.");
    expect(screen.getByLabelText("Concurso")).toHaveValue("TRT");
    expect(within(screen.getByLabelText("Concurso")).getAllByRole("option")).toHaveLength(3);
  });

  it("breaks the trend line where a week has no answers", async () => {
    backend(data);
    render(() => <Performance />);
    const chart = await screen.findByRole("img", { name: "Acerto semanal" });
    // 9 and 23 November are two weeks apart: two single-point segments, not one line.
    expect(chart.querySelectorAll("polyline")).toHaveLength(2);
  });

  it("offers the weekly numbers as a table for screen readers", async () => {
    backend(data);
    render(() => <Performance />);
    const table = await screen.findByRole("table", { name: "Acerto semanal" });
    const rows = within(table).getAllByRole("row").slice(1);
    expect(rows[0]).toHaveTextContent("09/11");
    expect(rows[1]).toHaveTextContent("75%");
  });

  it("explains an empty bank", async () => {
    backend(emptyPerformance);
    render(() => (
      <HashRouter>
        <Route path="/" component={Performance} />
      </HashRouter>
    ));
    expect(await screen.findByText("Nenhuma questão no banco ainda.")).toBeVisible();
    expect(screen.getByRole("link", { name: "Gerar questões" })).toHaveAttribute("href", "#/gerar");
  });

  it("explains when nothing was finished yet", async () => {
    backend({ ...emptyPerformance, overall: stats(0, 0, 0, 12) });
    render(() => (
      <HashRouter>
        <Route path="/" component={Performance} />
      </HashRouter>
    ));
    expect(await screen.findByText("Você ainda não finalizou nenhuma tentativa.")).toBeVisible();
    expect(screen.getAllByRole("link", { name: "Ir para o Banco" })).toHaveLength(2);
    expect(screen.getByText("Ainda não há respostas para mostrar a evolução.")).toBeVisible();
    expect(screen.queryByRole("heading", { name: "Assuntos mais fracos" })).not.toBeInTheDocument();
  });

  it("shows the backend's message when loading fails", async () => {
    backend({ error: "Falha ao calcular." });
    render(() => <Performance />);
    expect(await screen.findByRole("alert")).toHaveTextContent("Falha ao calcular.");
  });
});
