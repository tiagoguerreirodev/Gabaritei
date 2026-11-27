import { InlineError } from "../../components/states";
import { For, Show } from "solid-js";
import type { Role } from "../../bindings/Role";
import { refreshCatalog, type RoleEditor } from "../../catalog";
import {
  AgentField,
  CatalogNotice,
  EffortField,
  FastField,
  ModelField,
} from "../../components/ModelOptions";
import { roleLabels } from "../../labels";

/** Agent, model, effort for each of the four roles. */
export default function RolesSection(props: {
  editors: Array<{ role: Role; editor: RoleEditor }>;
}) {
  return (
    <section class="card card-wide" aria-labelledby="roles-title">
      <h2 id="roles-title">Funções</h2>
      <p class="muted">
        Cada função usa um agente, um modelo e um esforço de raciocínio. Os modelos e esforços vêm
        dos próprios agentes. Na tela de geração dá para trocar o gerador e o conferente só daquela
        vez.
      </p>
      <p>
        <button type="button" class="btn" onClick={() => void refreshCatalog()}>
          Atualizar listas
        </button>
      </p>
      <div class="table-scroll" role="region" tabindex="0" aria-labelledby="roles-title">
        <table class="data-table is-text">
          <thead>
            <tr>
              <th scope="col">Função</th>
              <th scope="col">Agente</th>
              <th scope="col">Modelo</th>
              <th scope="col">Esforço</th>
            </tr>
          </thead>
          <tbody>
            <For each={props.editors}>
              {({ role, editor }) => {
                const name = roleLabels[role].name;
                return (
                  <tr>
                    <th scope="row">
                      {name}
                      <span class="muted caption">{roleLabels[role].hint}</span>
                    </th>
                    <td>
                      <AgentField label={name} editor={editor} />
                    </td>
                    <td>
                      <ModelField label={name} editor={editor} />
                    </td>
                    <td>
                      <EffortField label={name} editor={editor} />
                      <FastField label={name} editor={editor} />
                    </td>
                  </tr>
                );
              }}
            </For>
          </tbody>
        </table>
      </div>
      <CatalogNotice
        agents={[...new Set(props.editors.map(({ editor }) => editor.value().agent))]}
      />
      <For each={props.editors}>
        {({ role, editor }) => (
          <Show when={editor.options()?.problem}>
            {(problem) => (
              <InlineError>
                {roleLabels[role].name}: {problem()}
              </InlineError>
            )}
          </Show>
        )}
      </For>
    </section>
  );
}
