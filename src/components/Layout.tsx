import { A } from "@solidjs/router";
import { For, Show, createResource, type ParentProps } from "solid-js";
import { api } from "../api";
import { routes, type AppRoute } from "../routes";
import { Toaster } from "./toasts";

function MenuLink(props: { route: AppRoute }) {
  return (
    <A href={props.route.path} end activeClass="active" class="menu-link">
      {props.route.label}
    </A>
  );
}

export default function Layout(props: ParentProps) {
  const [info] = createResource(() => api.appInfo());

  return (
    <div class="shell">
      <aside class="sidebar">
        <div class="brand">Gabaritei</div>
        <nav aria-label="Menu principal">
          <For each={routes.filter((route) => route.menu === "main")}>
            {(route) => <MenuLink route={route} />}
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
      <main class="content">{props.children}</main>
      <Toaster />
    </div>
  );
}
