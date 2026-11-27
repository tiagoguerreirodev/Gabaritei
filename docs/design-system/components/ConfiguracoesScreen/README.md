# ConfiguracoesScreen
Configurações: one card per concern, in the order people need them. Changes to choices apply at once; tests and paths report their status inline.

Composition: `h1` *Configurações*; *Aparência* (two ChoiceGroups: Tema, Tamanho do texto); *Agentes* (SettingsRows for Claude Code and Codex with *Testar*); *Papéis* (DataTable `.is-text` of role → model); *RemNote*; *Atalhos* (one checkbox); *Backups* (folder SettingsRow, `settings-list` of backups with *Restaurar*); *Dados* (data folder and file, with the one primary action *Fazer backup agora*).
