# ChoiceGroup
A small set of radio choices with a legend: theme and text size on Configurações, the order of the Desempenho table, session mode and order in Banco's ResultPanel. `fieldset.choice-group`.

- Stacked (default): `legend` (600) then one `label` per option, radio first. Add a muted line inside the label when an option needs explaining (*Simulado: o gabarito aparece no fim*).
- `.is-inline`: legend and options on one wrapping line, legend in `small` muted. Use it for a choice that only re-orders what is below it.
- Radios and checkboxes use the native control tinted with `accent-color: accent`. Two to five options; more is a Field `select`.
- Changing the choice applies at once (no *Salvar*), except inside a form that has its own submit.
- In `.is-inline`, each `label` centres its radio with its text, `space-1` apart; the group sits `space-3` above what follows.
