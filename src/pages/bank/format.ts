/** pt-BR number, e.g. 9.368. */
export const formatNumber = (n: number) => n.toLocaleString("pt-BR");

/** pt-BR date, e.g. 19/11/2026. */
export const formatDate = (iso: string) => new Date(iso).toLocaleDateString("pt-BR");

/** "1 questão" / "9.368 questões". */
export const questionCount = (n: number) =>
  `${formatNumber(n)} ${n === 1 ? "questão" : "questões"}`;
