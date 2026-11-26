import { HashRouter } from "@solidjs/router";
import { render, screen, waitFor } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { resetGenerationWatch } from "../generation";
import { routeDefinitions, routes } from "../routes";
import Layout from "./Layout";

const idle = {
  revision: 0,
  runRevision: 0,
  running: false,
  startedAt: null,
  progress: null,
  outcome: null,
};

/** An open Simulado attempt with one unanswered question. */
const openAttempt = {
  id: "att",
  sourceKind: "collection",
  sourceRef: "c1",
  title: "Caderno de teste",
  feedbackMode: "exam",
  startedAt: "2026-11-20T10:00:00.000Z",
  finishedAt: null,
  revision: 0,
  questions: [
    {
      position: 0,
      questionId: "q0",
      type: "boolean",
      stem: "Enunciado",
      options: [
        { id: "c", text: "Certo" },
        { id: "e", text: "Errado" },
      ],
      image: null,
      imageAlt: null,
      choice: null,
      eliminated: [],
      flagged: false,
      reveal: null,
    },
  ],
  score: null,
};

function backend(waiting: unknown[] = []) {
  mockIPC(
    (command) => {
      switch (command) {
        case "attempt":
          return openAttempt;
        case "keyboard_shortcuts":
          return true;
        case "generation_status":
          return idle;
        case "drafts_waiting":
          return waiting;
        case "app_info":
          return { name: "Gabaritei", version: "0.2.0" };
        default:
          // Page data is not under test here: a request that never answers keeps pages loading.
          return new Promise(() => {});
      }
    },
    { shouldMockEvents: true },
  );
}

function renderAt(path: string) {
  window.location.hash = `#${path}`;
  resetGenerationWatch();
  render(() => <HashRouter root={Layout}>{routeDefinitions}</HashRouter>);
}

beforeEach(() => {
  window.location.hash = "";
  localStorage.clear();
  backend();
});

afterEach(() => {
  // jsdom has no matchMedia of its own.
  Reflect.deleteProperty(window, "matchMedia");
  vi.restoreAllMocks();
});

const draft = (id: string) => ({
  id,
  topic: { area: "A", matter: "M", topic: "T" },
  count: 3,
  createdAt: "2026-01-01T00:00:00Z",
  disputed: 0,
  checking: false,
});

describe("Sidebar", () => {
  it("keeps focus on the toggle, and its title follows the state", async () => {
    const user = userEvent.setup();
    renderAt("/");
    const toggle = screen.getByRole("button", { name: "Menu lateral" });
    expect(toggle).toHaveAttribute("title", "Recolher menu");
    await user.click(toggle);
    expect(toggle).toHaveFocus();
    expect(toggle).toHaveAttribute("title", "Expandir menu");
    await user.click(toggle);
    expect(toggle).toHaveFocus();
    expect(toggle).toHaveAttribute("title", "Recolher menu");
  });

  it("renders expanded and still toggles when storage throws", async () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    const user = userEvent.setup();
    renderAt("/");
    const toggle = screen.getByRole("button", { name: "Menu lateral" });
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(document.querySelector(".shell")).toHaveAttribute("data-nav", "collapsed");
  });

  it("names the review link with its count, expanded and collapsed", async () => {
    backend([draft("a"), draft("b")]);
    const user = userEvent.setup();
    renderAt("/");
    const link = await screen.findByRole("link", { name: "Revisar pacote, 2 esperando" });
    expect(link).not.toHaveAttribute("title");
    expect(link.querySelector(".nav-label-extra")).toHaveTextContent(", 2 esperando");
    await user.click(screen.getByRole("button", { name: "Menu lateral" }));
    expect(screen.getByRole("link", { name: "Revisar pacote, 2 esperando" })).toBeInTheDocument();
  });

  it("is a toggle with aria-expanded and aria-controls that remembers the choice", async () => {
    const user = userEvent.setup();
    renderAt("/");
    const toggle = screen.getByRole("button", { name: "Menu lateral" });
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    const controlled = document.getElementById(toggle.getAttribute("aria-controls")!);
    expect(controlled).toContainElement(screen.getByRole("link", { name: "Banco" }));

    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(document.querySelector(".shell")).toHaveAttribute("data-nav", "collapsed");
    expect(localStorage.getItem("gabaritei.sidebarCollapsed")).toBe("1");
    // Labels stay available to assistive technology while collapsed.
    expect(screen.getByRole("link", { name: "Banco" })).toBeInTheDocument();

    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(document.querySelector(".shell")).not.toHaveAttribute("data-nav", "collapsed");
  });

  it("collapses the nav and hides the toggle when the window is narrower than 60rem", () => {
    const matchMedia = vi.fn().mockReturnValue({
      matches: true,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    });
    renderAt("/");
    expect(window.matchMedia).toHaveBeenCalledWith("(max-width: 60rem)");
    expect(document.querySelector(".shell")).toHaveAttribute("data-nav", "collapsed");
    expect(screen.queryByRole("button", { name: "Menu lateral" })).not.toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Banco" })).toBeInTheDocument();
  });

  it("starts collapsed when that was the saved choice", () => {
    localStorage.setItem("gabaritei.sidebarCollapsed", "1");
    renderAt("/");
    expect(screen.getByRole("button", { name: "Menu lateral" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
  });

  it("marks only the current page and hides the icons from assistive technology", async () => {
    const user = userEvent.setup();
    renderAt("/banco");
    await waitFor(() =>
      expect(screen.getByRole("link", { name: "Banco" })).toHaveAttribute("aria-current", "page"),
    );
    expect(screen.getByRole("link", { name: "Início" })).not.toHaveAttribute("aria-current");
    expect(document.querySelectorAll("nav svg:not([aria-hidden='true'])")).toHaveLength(0);
    expect(document.querySelectorAll("nav svg").length).toBeGreaterThan(0);

    await user.click(screen.getByRole("link", { name: "Gerar" }));
    expect(screen.getByRole("link", { name: "Gerar" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("link", { name: "Banco" })).not.toHaveAttribute("aria-current");
  });
});

describe("Focus mode", () => {
  const sidebar = () => document.querySelector("aside.nav");

  it("hides the sidebar while an attempt is open, until Modo foco is turned off", async () => {
    const user = userEvent.setup();
    renderAt("/resolver?tentativa=att");
    const button = await screen.findByRole("button", { name: "Modo foco" });
    expect(sidebar()).toHaveAttribute("hidden");
    expect(document.querySelector(".shell")).toHaveClass("focus-mode");
    expect(screen.queryByRole("navigation", { name: "Menu principal" })).not.toBeInTheDocument();

    await user.click(button);
    expect(sidebar()).not.toHaveAttribute("hidden");
    expect(document.querySelector(".shell")).not.toHaveClass("focus-mode");
    expect(screen.getByRole("navigation", { name: "Menu principal" })).toBeInTheDocument();
  });

  it("keeps the sidebar on other pages", () => {
    renderAt("/banco");
    expect(sidebar()).not.toHaveAttribute("hidden");
    expect(document.querySelector(".shell")).not.toHaveClass("focus-mode");
  });
});

describe("one h1 per route", () => {
  const paths = [...routes.map((route) => route.path), "/nao-existe"];
  it.each(paths)("%s has exactly one heading of level 1", async (path) => {
    renderAt(path);
    await waitFor(() => expect(screen.getAllByRole("heading", { level: 1 })).toHaveLength(1));
  });
});
