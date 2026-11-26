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
import { stats } from "../test/fixtures";
import Bank from "./Bank";

const tree: TopicTree = {
  questions: 9368,
  areas: [
    {
      name: "Direito",
      stats: stats(30, 10, 0, 1200),
      matters: [
        {
          name: "Constitucional",
          stats: stats(30, 10, 0, 300),
          topics: [
            {
              topicId: 1,
              name: "Controle de constitucionalidade",
              stats: stats(30, 10, 0, 120),
              trend: [],
            },
            { topicId: 2, name: "Direitos fundamentais", stats: stats(0, 0, 0, 1), trend: [] },
          ],
        },
      ],
    },
    { name: "Informática", stats: stats(0, 0, 0, 2400), matters: [] },
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
  lastAchievement: null,
  activeAttemptId: null,
});

// 23 collections: two pages of 20.
const collections: CollectionSummary[] = [
  {
    ...summary("c-adm", "Direito › Administração", "ALEPA · Programação", null, 41),
    lastFinishedAt: "2026-11-20T12:00:00.000Z",
    lastAchievement: 0.45,
  },
  summary("c-sql", "Informática › SQL", "Perito Criminal", "FGV"),
  {
    ...summary("c-crase", "Português › Crase", "ALEPA · Programação", "Cebraspe"),
    lastFinishedAt: "2026-11-18T12:00:00.000Z",
    lastAchievement: 0,
    activeAttemptId: "att-crase",
  },
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
    if (command === "question_count_limits") {
      const { available } = (args as { context: { available: number } }).context;
      return { min: 1, max: Math.min(200, available), initial: Math.min(10, available) };
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

    it("shows coverage and an achievement bar on every level", async () => {
      backend();
      const user = userEvent.setup();
      render(() => <Bank />);

      const treeList = await screen.findByRole("list", { name: "Assuntos" });
      const direito = within(treeList).getByText("Direito").closest("summary")!;
      expect(direito).toHaveTextContent("cobertura 3,3%");
      expect(
        within(direito).getByRole("meter", { name: "Aproveitamento em Direito" }),
      ).toHaveAttribute("aria-valuetext", "75% de aproveitamento");

      await user.click(within(treeList).getByText("Direito"));
      await user.click(within(treeList).getByText("Constitucional"));
      const controle = within(treeList).getByText("Controle de constitucionalidade").closest("li")!;
      expect(controle).toHaveTextContent("cobertura 33,3%");
      const fundamentais = within(treeList).getByText("Direitos fundamentais").closest("li")!;
      expect(fundamentais).toHaveTextContent("nunca resolvido");
      expect(fundamentais).not.toHaveTextContent("cobertura");
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
    const card = (title: string) =>
      screen.getByRole("button", { name: title }).closest<HTMLElement>(".collection-card")!;

    it("lists the first page with counts, tags and the last attempt", async () => {
      const queries = backend();
      await openCollections();

      expect(screen.getByText("23 cadernos")).toBeVisible();
      expect(listedTitles()).toHaveLength(20);
      const first = card("Direito › Administração");
      expect(first).toHaveTextContent("ALEPA · Programação");
      expect(first).toHaveTextContent("41 questões");
      expect(first).toHaveTextContent("Última tentativa em 20/11/2026");
      expect(within(first).getByText("45% de aproveitamento")).toHaveAttribute("data-band", "weak");
      expect(card("Informática › SQL")).toHaveTextContent("Nunca resolvido");
      expect(queries[0]).toEqual({ search: "", contest: "", board: "", page: 0, pageSize: 20 });
    });

    it("shows a 0% achievement instead of hiding it", async () => {
      backend();
      await openCollections();

      expect(card("Português › Crase")).toHaveTextContent("0% de aproveitamento");
    });

    it("starts a collection session from its card in one click", async () => {
      const calls = backend();
      const user = await openCollections();

      await user.click(screen.getByRole("button", { name: "Resolver Informática › SQL" }));
      const start = await screen.findByRole("dialog", { name: "Informática › SQL" });
      await user.click(within(start).getByRole("button", { name: "Começar" }));

      await waitFor(() => expect(calls.started).toHaveLength(1));
      expect(calls.started[0]).toEqual({
        source: { kind: "collection", collectionId: "c-sql" },
        feedbackMode: "study",
      });
    });

    it("continues the attempt in progress from its card", async () => {
      backend();
      const user = await openCollections();
      const crase = card("Português › Crase");

      expect(crase).toHaveTextContent("Em andamento");
      expect(within(crase).queryByRole("button", { name: /^Resolver/ })).not.toBeInTheDocument();
      await user.click(within(crase).getByRole("button", { name: "Continuar Português › Crase" }));
      expect(window.location.hash).toBe("#/resolver?tentativa=att-crase");
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

    it("moves to the next page of collections", async () => {
      const queries = backend();
      const user = await openCollections();
      expect(screen.getByText("Página 1 de 2")).toBeVisible();
      expect(screen.getByRole("button", { name: "Anterior" })).toBeDisabled();

      await user.click(screen.getByRole("button", { name: "Próxima" }));
      expect(await screen.findByText("Página 2 de 2")).toBeVisible();
      expect(listedTitles()).toHaveLength(3);
      expect(screen.getByRole("button", { name: "Próxima" })).toBeDisabled();
      expect(queries.at(-1)?.page).toBe(1);
    });

    it("returns to the first collection page after a filter change", async () => {
      const queries = backend();
      const user = await openCollections();
      await user.click(screen.getByRole("button", { name: "Próxima" }));
      await screen.findByText("Página 2 de 2");

      await user.selectOptions(
        screen.getByRole("combobox", { name: "Concurso" }),
        "Perito Criminal",
      );

      expect(await screen.findByText("Página 1 de 2")).toBeVisible();
      expect(queries.at(-1)).toMatchObject({ contest: "Perito Criminal", page: 0 });
    });

    async function sqlCollection(user: Awaited<ReturnType<typeof openCollections>>) {
      await user.click(screen.getByRole("button", { name: "Informática › SQL" }));
      return screen.findByRole("dialog", { name: "Informática › SQL" });
    }

    it("shows the selected collection's details", async () => {
      backend();
      const user = await openCollections();
      const dialog = await sqlCollection(user);

      expect(await within(dialog).findByTestId("collection-questions")).toHaveTextContent(
        "30 questões",
      );
      expect(within(dialog).getByText("FGV")).toBeVisible();
      expect(within(dialog).getByText("Nenhuma tentativa ainda.")).toBeVisible();
    });

    it("returns to the collection details when the mode choice is cancelled", async () => {
      backend();
      const user = await openCollections();
      const dialog = await sqlCollection(user);

      await user.click(await within(dialog).findByRole("button", { name: "Iniciar" }));
      const start = await screen.findByRole("dialog", { name: "Informática › SQL" });
      await user.click(within(start).getByRole("button", { name: "Cancelar" }));

      const details = await screen.findByRole("dialog", { name: "Informática › SQL" });
      expect(within(details).getByText("Nenhuma tentativa ainda.")).toBeVisible();
    });

    it("starts a collection session in the chosen mode", async () => {
      const calls = backend();
      const user = await openCollections();
      const details = await sqlCollection(user);

      await user.click(await within(details).findByRole("button", { name: "Iniciar" }));
      const start = await screen.findByRole("dialog", { name: "Informática › SQL" });
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
      await user.click(screen.getByRole("button", { name: "Informática › SQL" }));
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
