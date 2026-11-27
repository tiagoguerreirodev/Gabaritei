import { describe, expect, it } from "vitest";
import { bancoHref, bancoSearch, emptyBancoState, emptyFilter, parseBanco } from "./filterUrl";
import type { BancoState } from "./filterUrl";

describe("Banco URL state", () => {
  it("round_trips_every_field", () => {
    const state: BancoState = {
      filter: {
        topicIds: [3, 5],
        statuses: ["wrong", "due"],
        collectionId: "c-1",
        attemptId: "a-1",
      },
      order: "random",
      mode: "exam",
    };
    expect(parseBanco(bancoSearch(state))).toEqual(state);
  });

  it("drops_unknown_values", () => {
    const parsed = parseBanco({
      status: "wrong,bogus",
      assuntos: "3,x,-1",
      ordem: "sideways",
      modo: "race",
    });
    expect(parsed).toEqual({
      filter: { ...emptyFilter, topicIds: [3], statuses: ["wrong"] },
      order: null,
      mode: "study",
    });
  });

  it("omits_empty_fields", () => {
    expect(bancoSearch(emptyBancoState)).toEqual({
      assuntos: undefined,
      status: undefined,
      caderno: undefined,
      tentativa: undefined,
      ordem: undefined,
      modo: undefined,
    });
  });

  it("builds_a_preset_link", () => {
    expect(
      bancoHref({ filter: { ...emptyFilter, statuses: ["due", "wrong"] }, order: "dueFirst" }),
    ).toBe("#/banco?status=due,wrong&ordem=dueFirst");
  });

  it("links_to_plain_banco_without_state", () => {
    expect(bancoHref({})).toBe("#/banco");
  });
});
