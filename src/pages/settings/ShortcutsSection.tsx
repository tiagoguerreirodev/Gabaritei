import { createSignal, onMount } from "solid-js";
import { loadShortcuts, saveShortcuts, shortcutsEnabled } from "../../shortcuts";
import { showError } from "../../components/toasts";

/** Atalhos de teclado: turns the Resolver's single-letter shortcuts on or off. */
export default function ShortcutsSection() {
  const [saving, setSaving] = createSignal(false);
  // Bumped after a failed save so the checkbox snaps back to the value still in effect.
  const [failures, setFailures] = createSignal(0);
  onMount(() => void loadShortcuts());

  const change = async (next: boolean) => {
    setSaving(true);
    try {
      await saveShortcuts(next);
    } catch (error) {
      showError(error);
      setFailures((count) => count + 1);
    } finally {
      setSaving(false);
    }
  };

  return (
    <section class="card" aria-labelledby="shortcuts-title">
      <h2 id="shortcuts-title">Resolver</h2>
      <label class="check-row">
        <input
          type="checkbox"
          checked={(failures(), shortcutsEnabled())}
          disabled={saving()}
          onChange={(event) => void change(event.currentTarget.checked)}
        />
        Atalhos de teclado
      </label>
      <p class="muted">
        Letras A–E (ou C/E), X e F respondem, eliminam e marcam questões sem o mouse. Desative se
        usa leitor de tela ou ditado por voz. As setas e o Enter continuam funcionando.
      </p>
    </section>
  );
}
