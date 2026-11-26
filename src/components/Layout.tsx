import { A, useLocation } from "@solidjs/router";
import {
  For,
  Show,
  createEffect,
  createResource,
  createSignal,
  on,
  onMount,
  type ParentProps,
} from "solid-js";
import { api } from "../api";
import { createMediaQuery } from "../mediaQuery";
import { focusMode } from "../focusMode";
import { routes, type AppRoute } from "../routes";
import { waitingDrafts, watchGeneration } from "../generation";
import GenerationBanner from "./GenerationBanner";
import { SidebarIcon, menuIcons } from "./icons";
import { Toaster } from "./toasts";

/** Below this width the nav is always icon-only. */
const NARROW_QUERY = "(max-width: 60rem)";
const COLLAPSED_KEY = "gabaritei.sidebarCollapsed";

function readCollapsed(): boolean {
  try {
    return localStorage.getItem(COLLAPSED_KEY) === "1";
  } catch {
    return false;
  }
}

function writeCollapsed(value: boolean) {
  try {
    localStorage.setItem(COLLAPSED_KEY, value ? "1" : "0");
  } catch {
    // Storage can be unavailable; the choice then lasts only for this session.
  }
}

function MenuLink(props: { route: AppRoute; count?: number }) {
  const name = () =>
    props.count ? `${props.route.label}, ${props.count} esperando` : props.route.label;
  return (
    <A href={props.route.path} end class="nav-item" aria-label={props.count ? name() : undefined}>
      {menuIcons[props.route.path]?.()}
      <span class="nav-label">
        {props.route.label}
        <Show when={props.count}>
          <span class="nav-label-extra">, {props.count} esperando</span>
        </Show>
      </span>
      <Show when={props.count}>
        <span class="badge badge-count" aria-hidden="true">
          {props.count}
        </span>
      </Show>
    </A>
  );
}

export default function Layout(props: ParentProps) {
  const [info] = createResource(() => api.appInfo());
  const location = useLocation();
  let main: HTMLElement | undefined;
  const [collapsed, setCollapsed] = createSignal(readCollapsed());
  const narrow = createMediaQuery(NARROW_QUERY);
  const iconOnly = () => collapsed() || narrow();
  const toggle = () => {
    const next = !collapsed();
    setCollapsed(next);
    writeCollapsed(next);
  };
  onMount(watchGeneration);

  // A route change moves focus to the page, so keyboard and screen-reader users start at the
  // top of the new content instead of on a link in the sidebar that is no longer relevant.
  createEffect(
    on(
      () => location.pathname,
      () => main?.focus({ preventScroll: true }),
      { defer: true },
    ),
  );

  return (
    <div
      class="shell"
      data-nav={iconOnly() ? "collapsed" : undefined}
      classList={{ "focus-mode": focusMode() }}
    >
      <button type="button" class="skip-link" onClick={() => main?.focus()}>
        Pular para o conteúdo
      </button>
      <aside class="nav" hidden={focusMode()}>
        <div class="nav-brand">
          <span class="nav-brand-name">Gabaritei</span>
          <Show when={!narrow()}>
            <button
              type="button"
              class="btn btn-ghost"
              title={collapsed() ? "Expandir menu" : "Recolher menu"}
              aria-expanded={!collapsed()}
              aria-controls="nav-menu"
              onClick={toggle}
            >
              <SidebarIcon />
              <span class="sr-only">Menu lateral</span>
            </button>
          </Show>
        </div>
        <div id="nav-menu" class="nav-menu">
          <nav aria-label="Menu principal">
            <For each={routes.filter((route) => route.menu === "main")}>
              {(route) => <MenuLink route={route} />}
            </For>
            <For each={routes.filter((route) => route.menu === "waiting")}>
              {(route) => (
                <Show when={waitingDrafts().length > 0}>
                  <MenuLink route={route} count={waitingDrafts().length} />
                </Show>
              )}
            </For>
          </nav>
          <div class="nav-footer">
            <nav aria-label="Menu secundário">
              <For each={routes.filter((route) => route.menu === "footer")}>
                {(route) => <MenuLink route={route} />}
              </For>
            </nav>
            <Show when={info()}>
              {(appInfo) => (
                <p class="nav-version" data-testid="app-version">
                  {appInfo().name} {appInfo().version}
                </p>
              )}
            </Show>
          </div>
        </div>
      </aside>
      <main class="content" id="main" tabindex="-1" ref={main}>
        <GenerationBanner />
        {props.children}
      </main>
      <Toaster />
    </div>
  );
}
