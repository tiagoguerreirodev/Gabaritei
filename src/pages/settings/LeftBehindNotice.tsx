import { Show } from "solid-js";
import type { LeftBehind } from "../../bindings/LeftBehind";

function availableOffer(offer: LeftBehind) {
  return offer.state === "available" ? offer : undefined;
}

export default function LeftBehindNotice(props: {
  offer: LeftBehind;
  busy: boolean;
  relocated: boolean;
  onCopy: () => void;
  onMove: () => void;
  onDismiss: () => void;
}) {
  return (
    <div class="card card-row notice" role="status" data-testid="left-behind">
      <Show
        when={availableOffer(props.offer)}
        fallback={
          <p>
            A pasta anterior (<code>{props.offer.folder}</code>) tinha backups que ficaram para
            trás, mas não está acessível agora (um disco desconectado ou uma pasta de rede fora do
            ar, por exemplo). Se ela voltar, a oferta de copiar ou mover aparece de novo aqui. Se
            não for mais voltar, dispense o aviso.
          </p>
        }
      >
        {(offer) => (
          <>
            <p>
              A pasta anterior (<code>{offer().folder}</code>) ainda tem{" "}
              {offer().backups === 1
                ? "1 backup, que não aparece mais aqui."
                : `${offer().backups} backups, que não aparecem mais aqui.`}
              <Show when={offer().safety > 0}>
                {" "}
                {safetyCopies(offer().backups, offer().safety)}; para voltar atrás, volte para
                aquela pasta.
              </Show>{" "}
              <Show
                when={props.relocated}
                fallback="Nada foi movido nem apagado. Se quiser, leve os backups para a pasta nova: cada cópia é conferida antes de qualquer original ser apagado. Os backups do outro computador, se houver, ficam onde estão."
              >
                O que não foi movido continua na pasta anterior. Você pode tentar de novo. Os
                backups levados passam a contar no limite da pasta nova (7 diários, 7 manuais e 5 de
                antes de restaurar).
              </Show>
            </p>
            <button type="button" class="btn" disabled={props.busy} onClick={() => props.onCopy()}>
              Copiar para a pasta nova
            </button>{" "}
            <button type="button" class="btn" disabled={props.busy} onClick={() => props.onMove()}>
              Mover para a pasta nova
            </button>{" "}
          </>
        )}
      </Show>
      <button type="button" class="btn" disabled={props.busy} onClick={() => props.onDismiss()}>
        Entendi
      </button>
    </div>
  );
}

function safetyCopies(backups: number, safety: number): string {
  if (backups === 1) return "Ele é uma cópia feita antes de restaurar ou importar";
  if (safety === 1) return "Um deles é uma cópia feita antes de restaurar ou importar";
  return `${safety} deles são cópias feitas antes de restaurar ou importar`;
}
