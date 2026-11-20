import { A } from "@solidjs/router";
import Placeholder from "../components/Placeholder";

export default function Generate() {
  return (
    <Placeholder
      title="Gerar"
      description="Escolha um assunto e a quantidade de questões; a IA elabora o pacote e outro modelo confere o gabarito às cegas."
    >
      <p>
        <A href="/revisar-pacote">Abrir a tela de revisão de pacote</A>
      </p>
    </Placeholder>
  );
}
