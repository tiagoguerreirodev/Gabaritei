import type { Component } from "solid-js";
import Bank from "./pages/Bank";
import Generate from "./pages/Generate";
import Home from "./pages/Home";
import Learning from "./pages/Learning";
import NotFound from "./pages/NotFound";
import Performance from "./pages/Performance";
import ReviewPackage from "./pages/ReviewPackage";
import Settings from "./pages/Settings";
import Solve from "./pages/Solve";

export interface AppRoute {
  path: string;
  /** pt-BR label used in the sidebar and page title. */
  label: string;
  component: Component;
  /**
   * Where the page appears in the sidebar. Contextual pages (opened from another screen,
   * like the package review after a generation) have no menu entry.
   */
  menu: "main" | "footer" | "none";
}

export const routes: AppRoute[] = [
  { path: "/", label: "Início", component: Home, menu: "main" },
  { path: "/banco", label: "Banco", component: Bank, menu: "main" },
  { path: "/gerar", label: "Gerar", component: Generate, menu: "main" },
  { path: "/revisar-pacote", label: "Revisar pacote", component: ReviewPackage, menu: "none" },
  { path: "/resolver", label: "Resolver", component: Solve, menu: "none" },
  { path: "/desempenho", label: "Desempenho", component: Performance, menu: "main" },
  { path: "/erros-e-revisoes", label: "Erros e revisões", component: Learning, menu: "main" },
  { path: "/configuracoes", label: "Configurações", component: Settings, menu: "footer" },
];

/** Route definitions for the router, including the catch-all page. */
export const routeDefinitions = [
  ...routes.map(({ path, component }) => ({ path, component })),
  { path: "*404", component: NotFound },
];
