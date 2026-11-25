import { InlineError } from "../../components/states";
import { For, Show } from "solid-js";
import type { Agent } from "../../bindings/Agent";
import type { RoleConfig } from "../../bindings/RoleConfig";
import type { RoleSettings } from "../../bindings/RoleSettings";
import { catalogState, refreshCatalog, roleForAgent, roleProblem } from "../../catalog";
import { CatalogNotice, EffortField, FastField, ModelField } from "../../components/ModelOptions";
import { agentLabels, agents, roleLabels, roleOrder } from "../../labels";

/** Agent, model, effort for each of the four roles. */
export default function RolesSection(props: {
  roles: RoleSettings;
  onChange: (key: keyof RoleSettings, config: RoleConfig) => void;
}) {
  return (
    <section class="card wide" aria-labelledby="roles-title">
      <h2 id="roles-title">Funções</h2>
      <p class="muted">
        Cada função usa um agente, um modelo e um esforço de raciocínio. Os modelos e esforços vêm
        dos próprios agentes. Na tela de geração você pode trocar o gerador e o conferente só
        daquela vez.
      </p>
      <p>
        <button type="button" onClick={() => void refreshCatalog()}>
          Atualizar listas
        </button>
      </p>
      <div class="table-scroll" role="region" tabindex="0" aria-labelledby="roles-title">
        <table class="roles-table">
          <thead>
            <tr>
              <th scope="col">Função</th>
              <th scope="col">Agente</th>
              <th scope="col">Modelo</th>
              <th scope="col">Esforço</th>
            </tr>
          </thead>
          <tbody>
            <For each={roleOrder}>
              {({ role, key }) => {
                const config = () => props.roles[key];
                const name = roleLabels[role].name;
                const change = (next: RoleConfig) => props.onChange(key, next);
                return (
                  <tr>
                    <th scope="row">
                      {name}
                      <span class="muted role-hint">{roleLabels[role].hint}</span>
                    </th>
                    <td>
                      <select
                        aria-label={`Agente: ${name}`}
                        value={config().agent}
                        onChange={(event) =>
                          change(
                            roleForAgent(
                              catalogState(),
                              event.currentTarget.value as Agent,
                              config(),
                            ),
                          )
                        }
                      >
                        <For each={agents}>
                          {(agent) => <option value={agent}>{agentLabels[agent]}</option>}
                        </For>
                      </select>
                    </td>
                    <td>
                      <ModelField label={name} value={config()} onChange={change} />
                    </td>
                    <td>
                      <EffortField label={name} value={config()} onChange={change} />
                      <FastField label={name} value={config()} onChange={change} />
                    </td>
                  </tr>
                );
              }}
            </For>
          </tbody>
        </table>
      </div>
      <CatalogNotice agents={[...new Set(roleOrder.map(({ key }) => props.roles[key].agent))]} />
      <For each={roleOrder}>
        {({ role, key }) => (
          <Show when={roleProblem(catalogState(), props.roles[key])}>
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
