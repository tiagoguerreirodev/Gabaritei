import { For } from "solid-js";
import type { Agent } from "../../bindings/Agent";
import type { Effort } from "../../bindings/Effort";
import type { RoleConfig } from "../../bindings/RoleConfig";
import type { RoleSettings } from "../../bindings/RoleSettings";
import { agentLabels, agents, effortLabels, efforts, roleLabels, roleOrder } from "./labels";

/** Agent, model and effort for each of the four roles. */
export default function RolesSection(props: {
  roles: RoleSettings;
  onChange: (key: keyof RoleSettings, config: RoleConfig) => void;
}) {
  return (
    <section class="card wide" aria-labelledby="roles-title">
      <h2 id="roles-title">Funções</h2>
      <p class="muted">
        Cada função usa um agente, um modelo e um esforço de raciocínio. Na tela de geração você
        pode trocar o gerador e o conferente só daquela vez.
      </p>
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
                        props.onChange(key, {
                          ...config(),
                          agent: event.currentTarget.value as Agent,
                        })
                      }
                    >
                      <For each={agents}>
                        {(agent) => <option value={agent}>{agentLabels[agent]}</option>}
                      </For>
                    </select>
                  </td>
                  <td>
                    <input
                      type="text"
                      aria-label={`Modelo: ${name}`}
                      value={config().model}
                      onInput={(event) =>
                        props.onChange(key, { ...config(), model: event.currentTarget.value })
                      }
                    />
                  </td>
                  <td>
                    <select
                      aria-label={`Esforço: ${name}`}
                      value={config().effort}
                      onChange={(event) =>
                        props.onChange(key, {
                          ...config(),
                          effort: event.currentTarget.value as Effort,
                        })
                      }
                    >
                      <For each={efforts}>
                        {(effort) => <option value={effort}>{effortLabels[effort]}</option>}
                      </For>
                    </select>
                  </td>
                </tr>
              );
            }}
          </For>
        </tbody>
      </table>
    </section>
  );
}
