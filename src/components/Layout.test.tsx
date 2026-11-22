import { HashRouter } from "@solidjs/router";
import { render, screen, waitFor } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { beforeEach, describe, expect, it } from "vitest";
import { resetGenerationWatch } from "../generation";
import { routeDefinitions, routes } from "../routes";
import Layout from "./Layout";

const idle = { running: false, startedAt: null, progress: null, outcome: null };

function backend() {
  mockIPC((command) => {
    switch (command) {
      case "generation_status":
        return idle;
      case "drafts_waiting":
        return [];
      case "app_info":
        return { name: "Gabaritei", version: "0.2.0" };
      default:
        // Page data is not under test here: a request that never answers keeps pages loading.
        return new Promise(() => {});
    }
  });
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

describe("Sidebar", () => {
  it("is a toggle with aria-expanded and aria-controls that remembers the choice", async () => {
    const user = userEvent.setup();
    renderAt("/");
    const toggle = screen.getByRole("button", { name: "Menu lateral" });
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    const controlled = document.getElementById(toggle.getAttribute("aria-controls")!);
    expect(controlled).toContainElement(screen.getByRole("link", { name: "Banco" }));

    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(document.querySelector(".shell")).toHaveClass("sidebar-collapsed");
    expect(localStorage.getItem("gabaritei.sidebarCollapsed")).toBe("1");
    // Labels stay available to assistive technology while collapsed.
    expect(screen.getByRole("link", { name: "Banco" })).toBeInTheDocument();

    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(document.querySelector(".shell")).not.toHaveClass("sidebar-collapsed");
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

describe("one h1 per route", () => {
  const paths = [...routes.map((route) => route.path), "/nao-existe"];
  it.each(paths)("%s has exactly one heading of level 1", async (path) => {
    renderAt(path);
    await waitFor(() => expect(screen.getAllByRole("heading", { level: 1 })).toHaveLength(1));
  });
});
