import { A } from "@solidjs/router";
import { For, Show, createResource, onMount, type ParentProps } from "solid-js";
import { api } from "../api";
import { routes, type AppRoute } from "../routes";
import { waitingDrafts, watchGeneration } from "../generation";
import GenerationBanner from "./GenerationBanner";
import { Toaster } from "./toasts";

function MenuLink(props: { route: AppRoute; count?: number }) {
  return (
    <A href={props.route.path} end activeClass="active" class="menu-link">
      {props.route.label}
      <Show when={props.count}>
        <span class="menu-count" aria-label={`${props.count} esperando`}>
          {props.count}
        </span>
      </Show>
    </A>
  );
}

export default function Layout(props: ParentProps) {
  const [info] = createResource(() => api.appInfo());
  onMount(watchGeneration);

  return (
    <div class="shell">
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
      <main class="content">
        <GenerationBanner />
        {props.children}
      </main>
      <Toaster />
    </div>
  );
}
