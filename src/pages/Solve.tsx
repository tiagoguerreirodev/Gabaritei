import { A, useNavigate, useSearchParams } from "@solidjs/router";
import { EmptyState, ResourceView } from "../components/states";
import Resolver from "./solve/Resolver";

/** Resolver page: `#/resolver?tentativa=<id>`. */
export default function Solve() {
  const [params] = useSearchParams<{ tentativa?: string }>();
  const navigate = useNavigate();

  return (
    <section class="page">
      <ResourceView
        value={params.tentativa || undefined}
        pending={false}
        empty={
          <>
            <h1>Resolver</h1>
            <EmptyState
              title="Nenhuma tentativa aberta."
              action={
                <A class="button-link primary" href="/banco">
                  Ir para o Banco
                </A>
              }
            >
              <p class="muted">Comece um caderno ou um assunto no Banco.</p>
            </EmptyState>
          </>
        }
      >
        {(id) => <Resolver attemptId={id()} onLeave={() => navigate("/banco")} />}
      </ResourceView>
    </section>
  );
}
