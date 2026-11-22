import { A, useLocation } from "@solidjs/router";
import { For, Show, createEffect, createResource, on, onMount, type ParentProps } from "solid-js";
import { api } from "../api";
import { routes, type AppRoute } from "../routes";
import { waitingDrafts, watchGeneration } from "../generation";
import GenerationBanner from "./GenerationBanner";
import { Toaster } from "./toasts";

function MenuLink(props: { route: AppRoute; count?: number }) {
  return (
    <A
      href={props.route.path}
      end
      activeClass="active"
      class="menu-link"
      aria-label={props.count ? `${props.route.label}, ${props.count} esperando` : undefined}
    >
      {props.route.label}
      <Show when={props.count}>
        <span class="menu-count" aria-hidden="true">
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
    <div class="shell">
      <button type="button" class="skip-link" onClick={() => main?.focus()}>
        Pular para o conteúdo
      </button>
      <aside class="sidebar">
        <div class="brand">Gabaritei</div>
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
        <div class="sidebar-footer">
          <nav aria-label="Menu secundário">
            <For each={routes.filter((route) => route.menu === "footer")}>
              {(route) => <MenuLink route={route} />}
            </For>
          </nav>
          <Show when={info()}>
            {(appInfo) => (
              <p class="version" data-testid="app-version">
                {appInfo().name} {appInfo().version}
              </p>
            )}
          </Show>
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
