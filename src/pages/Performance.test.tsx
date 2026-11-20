import { render, screen, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { describe, expect, it } from "vitest";
import type { Performance as PerformanceData } from "../bindings/Performance";
import type { PerformanceQuery } from "../bindings/PerformanceQuery";
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
          topics: [{ topicId: 3, name: "Crase", stats: crase }],
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
          topics: [{ topicId: 1, name: "Controle", stats: direito }],
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
  const queries: PerformanceQuery[] = [];
  const exports: PerformanceQuery[] = [];
  mockIPC((command, args) => {
    const query = (args as { query: PerformanceQuery }).query;
    if (command === "performance") {
      queries.push(query);
      if ("error" in result) {
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        return Promise.reject({ message: result.error });
      }
      return result;
    }
    if (command === "export_performance_csv") {
      exports.push(query);
      return exported;
    }
    throw new Error(`Unexpected command: ${command}`);
  });
  return { queries, exports };
}

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

    const table = await screen.findByRole("table");
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

    await screen.findByRole("list", { name: "Resumo" });
    expect(queries.at(-1)).toMatchObject({
      contest: "TRT",
      board: "FGV",
      feedbackMode: "exam",
    });
  });

  it("exports the CSV with the active filters", async () => {
    const { exports } = backend(data);
    const user = userEvent.setup();
    render(() => <Performance />);
    await screen.findByRole("list", { name: "Resumo" });

    await user.selectOptions(screen.getByLabelText("Concurso"), "INSS");
    await user.click(screen.getByRole("button", { name: "Exportar CSV" }));

    await screen.findByRole("list", { name: "Resumo" });
    expect(exports).toHaveLength(1);
    expect(exports[0]).toMatchObject({ contest: "INSS" });
  });

  it("explains an empty bank", async () => {
    backend(emptyPerformance);
    render(() => <Performance />);
    expect(await screen.findByText("Nenhuma questão no banco ainda.")).toBeVisible();
  });

  it("explains when nothing was finished yet", async () => {
    backend({ ...emptyPerformance, overall: stats(0, 0, 0, 12) });
    render(() => <Performance />);
    expect(await screen.findByText("Você ainda não finalizou nenhuma tentativa.")).toBeVisible();
    expect(screen.getByText("Ainda não há respostas para mostrar a evolução.")).toBeVisible();
    expect(screen.queryByRole("heading", { name: "Assuntos mais fracos" })).not.toBeInTheDocument();
  });

  it("shows the backend's message when loading fails", async () => {
    backend({ error: "Falha ao calcular." });
    render(() => <Performance />);
    expect(await screen.findByRole("alert")).toHaveTextContent("Falha ao calcular.");
  });
});
