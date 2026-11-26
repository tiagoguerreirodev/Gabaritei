import { Navigate } from "@solidjs/router";
import type { Component } from "solid-js";
import Bank from "./pages/Bank";
import Generate from "./pages/Generate";
import Home from "./pages/Home";
import { bancoPath } from "./pages/bank/filterUrl";
import { presets } from "./pages/bank/presets";
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
   * Where the page appears in the sidebar. Contextual pages (opened from another screen) have
   * no menu entry. `waiting` pages show in the main menu, with a count, only while something
   * waits for the user (the package review, once a generation has produced a draft).
   */
  menu: "main" | "footer" | "waiting" | "none";
}

export const routes: AppRoute[] = [
  { path: "/", label: "Início", component: Home, menu: "main" },
  { path: "/banco", label: "Banco", component: Bank, menu: "main" },
  { path: "/gerar", label: "Gerar", component: Generate, menu: "main" },
  { path: "/revisar-pacote", label: "Revisar pacote", component: ReviewPackage, menu: "waiting" },
  { path: "/resolver", label: "Resolver", component: Solve, menu: "none" },
  { path: "/desempenho", label: "Desempenho", component: Performance, menu: "main" },
  { path: "/configuracoes", label: "Configurações", component: Settings, menu: "footer" },
];

/** Route definitions for the router, including the catch-all page. */
export const routeDefinitions = [
  ...routes.map(({ path, component }) => ({ path, component })),
  // Old bookmarks of the removed Erros e revisões page land on the reviews shortcut.
  { path: "/erros-e-revisoes", component: () => <Navigate href={bancoPath(presets.reviews)} /> },
  { path: "*404", component: NotFound },
];
