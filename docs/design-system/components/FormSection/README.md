# FormSection
A titled group of stacked Fields, used to break the Gerar form into steps you read top to bottom: *1 Assunto*, *2 Quantidade e banca*, *3 Fontes*, *4 Avançado*.

- Markup: `fieldset.form-section` > `legend.form-section-title.label` (number in `accent`, then the title, then a `rule` line filling the width), then Fields or a `.form-row`.
- Sections sit `space-8` apart inside `.form` (max `content-width`). *Avançado* is the last section, collapsed by default in a `details`.
- One primary action ends the form in `.form-actions` (*Gerar questões*), right-aligned over a `rule` line; secondary actions sit before it.
