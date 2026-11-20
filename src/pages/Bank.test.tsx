import { render, screen, waitFor, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { describe, expect, it } from "vitest";
import type { AttemptListItem } from "../bindings/AttemptListItem";
import type { CollectionDetail } from "../bindings/CollectionDetail";
import type { CollectionPage } from "../bindings/CollectionPage";
import type { CollectionQuery } from "../bindings/CollectionQuery";
import type { CollectionSummary } from "../bindings/CollectionSummary";
import type { TopicTree } from "../bindings/TopicTree";
import Bank from "./Bank";

const tree: TopicTree = {
  questions: 9368,
  areas: [
    {
      name: "Direito",
      questions: 1200,
      matters: [
        {
          name: "Constitucional",
          questions: 300,
          topics: [
            { id: 1, name: "Controle de constitucionalidade", questions: 120, accuracy: null },
            { id: 2, name: "Direitos fundamentais", questions: 1, accuracy: null },
          ],
        },
      ],
    },
    { name: "Informática", questions: 2400, matters: [] },
  ],
};

const summary = (
  id: string,
  title: string,
  contest: string | null,
  board: string | null,
  questions = 30,
): CollectionSummary => ({
  id,
  title,
  kind: "imported",
  contest,
  board,
  questions,
  createdAt: "2026-11-11T00:15:54.786Z",
  lastFinishedAt: null,
  inProgress: false,
});

// 23 collections: two pages of 20.
const collections: CollectionSummary[] = [
  summary("c-adm", "Direito › Administração", "ALEPA · Programação", null, 41),
  summary("c-sql", "Informática › SQL", "Perito Criminal", "FGV"),
  summary("c-crase", "Português › Crase", "ALEPA · Programação", "Cebraspe"),
  ...Array.from({ length: 20 }, (_, i) =>
    summary(`c-${i}`, `Assunto ${String(i).padStart(2, "0")}`, "Perito Criminal", null),
  ),
];

const fold = (text: string) =>
  text
    .normalize("NFD")
    .replace(/\p{Mn}/gu, "")
    .toLowerCase();

/** A fake backend that filters and paginates like the real one, recording each query. */
function backend(
  options: { tree?: TopicTree; failCollections?: string; attempts?: AttemptListItem[] } = {},
) {
  const queries: CollectionQuery[] = [];
  const started: unknown[] = [];
  mockIPC((command, args) => {
    if (command === "session_start") {
      started.push((args as { request: unknown }).request);
      return { id: "att-1" };
    }
    if (command === "topics_tree") return options.tree ?? tree;
    if (command === "collections") {
      if (options.failCollections) {
        // The backend rejects with a plain `ErrorPayload` object.
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        return Promise.reject({ message: options.failCollections });
      }
      const query = (args as { query: CollectionQuery }).query;
      queries.push(query);
      const matching = collections.filter(
        (c) =>
          fold(c.title).includes(fold(query.search.trim())) &&
          (!query.contest || c.contest === query.contest) &&
          (!query.board || c.board === query.board),
      );
      const page: CollectionPage = {
        items: matching.slice(query.page * query.pageSize, (query.page + 1) * query.pageSize),
        total: matching.length,
        page: query.page,
        pageSize: query.pageSize,
        contests: ["ALEPA · Programação", "Perito Criminal"],
        boards: ["Cebraspe", "FGV"],
      };
      return page;
    }
    if (command === "collection") {
      const id = (args as { id: string }).id;
      const found = collections.find((c) => c.id === id)!;
      const detail: CollectionDetail = {
        id,
        title: found.title,
        kind: found.kind,
        contest: found.contest,
        board: found.board,
        questions: found.questions,
        createdAt: found.createdAt,
        attempts: options.attempts ?? [],
      };
      return detail;
    }
    throw new Error(`Unexpected command: ${command}`);
  });
  return Object.assign(queries, { started });
}

const listedTitles = () =>
  within(screen.getByRole("list", { name: "Cadernos" }))
    .getAllByRole("listitem")
    .map((item) => item.querySelector(".collection-title")?.textContent);

async function openCollections() {
  const user = userEvent.setup();
  render(() => <Bank />);
  await user.click(screen.getByRole("tab", { name: "Cadernos" }));
  await screen.findByRole("list", { name: "Cadernos" });
  return user;
}

describe("Banco", () => {
  describe("topic tree", () => {
    it("shows areas, matters and topics with their question counts", async () => {
      backend();
      const user = userEvent.setup();
      render(() => <Bank />);

      const treeList = await screen.findByRole("list", { name: "Assuntos" });
      expect(screen.getByText("9.368 questões em 2 áreas.")).toBeVisible();
      expect(within(treeList).getByText("Direito").closest("summary")).toHaveTextContent(
        "1.200 questões",
      );

      await user.click(within(treeList).getByText("Direito"));
      await user.click(within(treeList).getByText("Constitucional"));
      const leaf = within(treeList).getByText("Direitos fundamentais").closest("li")!;
      expect(leaf).toHaveTextContent("1 questão");
      expect(within(treeList).getByText("Controle de constitucionalidade")).toBeVisible();
    });

    it("shows each answered topic's accuracy", async () => {
      const [direito, informatica] = tree.areas;
      const [constitucional] = direito!.matters;
      const [controle, fundamentais] = constitucional!.topics;
      backend({
        tree: {
          ...tree,
          areas: [
            {
              ...direito!,
              matters: [
                {
                  ...constitucional!,
                  topics: [{ ...controle!, accuracy: 0.5 }, fundamentais!],
                },
              ],
            },
            informatica!,
          ],
        },
      });
      const user = userEvent.setup();
      render(() => <Bank />);

      const treeList = await screen.findByRole("list", { name: "Assuntos" });
      await user.click(within(treeList).getByText("Direito"));
      await user.click(within(treeList).getByText("Constitucional"));
      expect(
        within(treeList).getByText("Controle de constitucionalidade").closest("li"),
      ).toHaveTextContent("acerto 50%");
      expect(
        within(treeList).getByText("Direitos fundamentais").closest("li"),
      ).not.toHaveTextContent("acerto");
    });

    it("explains an empty bank", async () => {
      backend({ tree: { areas: [], questions: 0 } });
      render(() => <Bank />);

      expect(await screen.findByText("Nenhuma questão no banco ainda.")).toBeVisible();
    });
  });

  describe("topic session", () => {
    it("starts a topic session with the chosen count", async () => {
      const calls = backend();
      const user = userEvent.setup();
      render(() => <Bank />);

      await user.click(
        await screen.findByRole("button", { name: "Resolver Controle de constitucionalidade" }),
      );
      const dialog = await screen.findByRole("dialog", {
        name: "Direito › Constitucional › Controle de constitucionalidade",
      });
      const count = within(dialog).getByRole("spinbutton", { name: /Quantidade/ });
      const begin = within(dialog).getByRole("button", { name: "Começar" });
      for (const invalid of ["2.5", "0", "121"]) {
        await user.clear(count);
        await user.type(count, invalid);
        expect(begin).toBeDisabled();
      }
      await user.clear(count);
      await user.type(count, "25");
      expect(begin).toBeEnabled();
      await user.click(within(dialog).getByRole("button", { name: "Começar" }));

      await waitFor(() => expect(calls.started).toHaveLength(1));
      expect(calls.started[0]).toEqual({
        source: { kind: "topic", topicId: 1, count: 25 },
        feedbackMode: "study",
      });
    });
  });

  describe("collections", () => {
    it("lists the first page with counts, tags and status", async () => {
      const queries = backend();
      await openCollections();

      expect(screen.getByText("23 cadernos")).toBeVisible();
      expect(listedTitles()).toHaveLength(20);
      const first = screen.getByRole("button", { name: /Direito › Administração/ });
      expect(first).toHaveTextContent("ALEPA · Programação");
      expect(first).toHaveTextContent("41 questões");
      expect(first).toHaveTextContent("Não iniciado");
      expect(queries[0]).toEqual({ search: "", contest: "", board: "", page: 0, pageSize: 20 });
    });

    it("searches ignoring case and accents", async () => {
      const queries = backend();
      const user = await openCollections();

      await user.type(screen.getByRole("searchbox", { name: "Buscar cadernos" }), "ADMINISTRACAO");

      expect(await screen.findByText("1 caderno")).toBeVisible();
      expect(listedTitles()).toEqual(["Direito › Administração"]);
      expect(queries.at(-1)?.search).toBe("ADMINISTRACAO");
    });

    it("explains when nothing matches the search", async () => {
      backend();
      const user = await openCollections();

      await user.type(screen.getByRole("searchbox", { name: "Buscar cadernos" }), "zzz");

      expect(await screen.findByText("Nenhum caderno encontrado.")).toBeVisible();
      expect(screen.getByText("Tente outra busca ou limpe os filtros.")).toBeVisible();
    });

    it("filters by contest and by board, together", async () => {
      const queries = backend();
      const user = await openCollections();

      await user.selectOptions(
        screen.getByRole("combobox", { name: "Concurso" }),
        "ALEPA · Programação",
      );
      expect(await screen.findByText("2 cadernos")).toBeVisible();
      expect(listedTitles()).toEqual(["Direito › Administração", "Português › Crase"]);

      await user.selectOptions(screen.getByRole("combobox", { name: "Banca" }), "Cebraspe");
      expect(await screen.findByText("1 caderno")).toBeVisible();
      expect(listedTitles()).toEqual(["Português › Crase"]);
      expect(queries.at(-1)).toMatchObject({ contest: "ALEPA · Programação", board: "Cebraspe" });
    });

    it("pages through results, and a new filter goes back to the first page", async () => {
      const queries = backend();
      const user = await openCollections();
      expect(screen.getByText("Página 1 de 2")).toBeVisible();
      expect(screen.getByRole("button", { name: "Anterior" })).toBeDisabled();

      await user.click(screen.getByRole("button", { name: "Próxima" }));
      expect(await screen.findByText("Página 2 de 2")).toBeVisible();
      expect(listedTitles()).toHaveLength(3);
      expect(screen.getByRole("button", { name: "Próxima" })).toBeDisabled();
      expect(queries.at(-1)?.page).toBe(1);

      await user.selectOptions(
        screen.getByRole("combobox", { name: "Concurso" }),
        "Perito Criminal",
      );
      expect(await screen.findByText("Página 1 de 2")).toBeVisible();
      expect(queries.at(-1)).toMatchObject({ contest: "Perito Criminal", page: 0 });
    });

    it("opens a collection's details and starts a session in the chosen mode", async () => {
      const calls = backend();
      const user = await openCollections();

      await user.click(screen.getByRole("button", { name: /Informática › SQL/ }));

      const dialog = await screen.findByRole("dialog", { name: "Informática › SQL" });
      expect(await within(dialog).findByTestId("collection-questions")).toHaveTextContent(
        "30 questões",
      );
      expect(within(dialog).getByText("FGV")).toBeVisible();
      expect(within(dialog).getByText("Nenhuma tentativa ainda.")).toBeVisible();
      // Cancelling the mode choice goes back to the collection's details.
      await user.click(within(dialog).getByRole("button", { name: "Iniciar" }));
      let start = await screen.findByRole("dialog", { name: "Informática › SQL" });
      await user.click(within(start).getByRole("button", { name: "Cancelar" }));
      const details = await screen.findByRole("dialog", { name: "Informática › SQL" });
      expect(within(details).getByText("Nenhuma tentativa ainda.")).toBeVisible();

      await user.click(within(details).getByRole("button", { name: "Iniciar" }));
      start = await screen.findByRole("dialog", { name: "Informática › SQL" });
      await user.click(within(start).getByRole("radio", { name: /Simulado/ }));
      await user.click(within(start).getByRole("button", { name: "Começar" }));

      await waitFor(() => expect(window.location.hash).toBe("#/resolver?tentativa=att-1"));
      expect(calls.started).toEqual([
        { source: { kind: "collection", collectionId: "c-sql" }, feedbackMode: "exam" },
      ]);
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });

    it("offers to continue an unfinished attempt instead of starting another", async () => {
      backend({
        attempts: [
          {
            id: "att-open",
            feedbackMode: "study",
            startedAt: "2026-11-20T10:00:00.000Z",
            finishedAt: null,
            answered: 3,
            total: 30,
          },
        ],
      });
      const user = await openCollections();
      await user.click(screen.getByRole("button", { name: /Informática › SQL/ }));
      const dialog = await screen.findByRole("dialog", { name: "Informática › SQL" });

      expect(await within(dialog).findByText("Há uma tentativa em andamento.")).toBeVisible();
      expect(within(dialog).queryByRole("button", { name: "Iniciar" })).not.toBeInTheDocument();
      await user.click(within(dialog).getAllByRole("button", { name: "Continuar" })[0]!);
      expect(window.location.hash).toBe("#/resolver?tentativa=att-open");

      await user.keyboard("{Escape}");
    });

    it("shows the backend's error when the list can't load", async () => {
      backend({ failCollections: "Não foi possível ler o banco." });
      const user = userEvent.setup();
      render(() => <Bank />);

      await user.click(screen.getByRole("tab", { name: "Cadernos" }));

      expect(await screen.findByRole("alert")).toHaveTextContent("Não foi possível ler o banco.");
    });
  });
});
