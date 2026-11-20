import { A } from "@solidjs/router";
import Placeholder from "../components/Placeholder";

export default function Bank() {
  return (
    <Placeholder
      title="Banco"
      description="Navegue pela árvore área › matéria › assunto e pelos cadernos, e inicie uma sessão de estudo."
    >
      <p>
        <A href="/resolver">Abrir a tela de resolução</A>
      </p>
    </Placeholder>
  );
}
