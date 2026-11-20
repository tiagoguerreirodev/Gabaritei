import { useNavigate, useSearchParams } from "@solidjs/router";
import { Show } from "solid-js";
import { EmptyState } from "../components/states";
import Resolver from "./solve/Resolver";

/** Resolver page: `#/resolver?tentativa=<id>`. */
export default function Solve() {
  const [params] = useSearchParams<{ tentativa?: string }>();
  const navigate = useNavigate();

  return (
    <section class="page">
      <Show
        when={params.tentativa}
        fallback={
          <>
            <h1>Resolver</h1>
            <EmptyState title="Nenhuma tentativa aberta.">
              <p class="muted">Comece um caderno ou um assunto no Banco.</p>
            </EmptyState>
          </>
        }
      >
        {(id) => <Resolver attemptId={id()} onLeave={() => navigate("/banco")} />}
      </Show>
    </section>
  );
}
