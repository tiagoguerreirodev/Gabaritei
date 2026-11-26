import { HashRouter, Route } from "@solidjs/router";
import { render, screen, waitFor } from "@solidjs/testing-library";
import { mockIPC } from "@tauri-apps/api/mocks";
import { describe, expect, it } from "vitest";
import { emptyHome, emptyPerformance } from "../test/fixtures";
import Bank from "./Bank";
import History from "./History";
import Home from "./Home";
import Learning from "./Learning";
import Performance from "./Performance";
import Settings from "./Settings";

// Every command fails, so each page shows its error state: the heading structure must not depend on data.
const pages = [
  ["Início", Home],
  ["Banco", Bank],
  ["Desempenho", Performance],
  ["Histórico", History],
  ["Erros e revisões", Learning],
  ["Configurações", Settings],
] as const;

describe("page headings", () => {
  it.each(pages)("%s has exactly one h1, with the page name", async (name, Page) => {
    mockIPC((command) =>
      command === "legacy_import_status"
        ? { report: null, bankEmpty: false }
        : // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
          Promise.reject({ message: "Falhou." }),
    );
    render(() => (
      <HashRouter>
        <Route path="/" component={Page} />
      </HashRouter>
    ));
    await waitFor(() => expect(screen.getAllByRole("alert").length).toBeGreaterThan(0));
    const h1s = screen.getAllByRole("heading", { level: 1 });
    expect(h1s).toHaveLength(1);
    expect(h1s[0]).toHaveTextContent(name);
  });

  it.each(pages)("%s has exactly one h1 once its data loaded", async (name, Page) => {
    const role = { agent: "codex", model: "m", effort: "high", fast: false };
    mockIPC((command) => {
      switch (command) {
        case "legacy_import_status":
          return { report: null, bankEmpty: false };
        case "home":
          return emptyHome;
        case "performance":
          return emptyPerformance;
        case "active_attempts":
          return [];
        case "attempt_history":
          return { items: [], total: 0, page: 0, pageSize: 20 };
        case "learning_overview":
          return {
            due: 0,
            dueInOpenAttempt: 0,
            upcoming: 0,
            errors: 0,
            practice: 0,
            practiceErrors: 0,
            items: [],
          };
        case "question_count_limits":
          return { min: 1, max: 30, initial: 10 };
        case "topics_tree":
          return { areas: [], questions: 0 };
        case "collections":
          return { items: [], total: 0, page: 0, pageSize: 20, contests: [], boards: [] };
        case "ai_settings":
          return {
            cliPaths: { claude: null, codex: null },
            roles: { generate: role, blindCheck: role, recheck: role, explain: role },
            remnote: { mode: "off", url: "", tools: [] },
            hasRemnoteToken: false,
          };
        case "backup_status":
          return {
            folder: "/dados/backups",
            isDefaultFolder: true,
            listing: { state: "ready", backups: [] },
            lastError: null,
            leftBehind: null,
          };
        case "data_folder":
          return "/dados";
        default:
          // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
          return Promise.reject({ message: "Falhou." });
      }
    });
    render(() => (
      <HashRouter>
        <Route path="/" component={Page} />
      </HashRouter>
    ));
    await waitFor(() => expect(screen.queryAllByRole("status")).toHaveLength(0));
    const h1s = screen.getAllByRole("heading", { level: 1 });
    expect(h1s).toHaveLength(1);
    expect(h1s[0]).toHaveTextContent(name);
  });
});
