import { For, createSignal } from "solid-js";
import type { Appearance } from "../../bindings/Appearance";
import { appearance, saveAppearance } from "../../appearance";
import { showError } from "../../components/toasts";

const THEMES: { value: Appearance["theme"]; label: string }[] = [
  { value: "system", label: "Automático (segue o sistema)" },
  { value: "light", label: "Claro" },
  { value: "dark", label: "Escuro" },
];

const TEXT_SIZES: { value: Appearance["textSize"]; label: string }[] = [
  { value: "s", label: "Pequeno" },
  { value: "m", label: "Médio" },
  { value: "l", label: "Grande" },
  { value: "xl", label: "Muito grande" },
];

/** Aparência: colour theme and text size. Each choice applies and saves at once. */
export default function AppearanceSection() {
  const [saving, setSaving] = createSignal(false);
  // Bumped after a failed save so the radios snap back to the choice that is still in effect.
  const [failures, setFailures] = createSignal(0);

  // The latest choice made while a save is pending; saved once the pending one ends.
  let queued: Appearance | undefined;

  const choose = async (next: Appearance) => {
    if (saving()) {
      queued = next;
      return;
    }
    setSaving(true);
    try {
      await saveAppearance(next);
    } catch (error) {
      showError(error);
      setFailures((count) => count + 1);
    } finally {
      setSaving(false);
    }
    const pending = queued;
    queued = undefined;
    if (pending) await choose(pending);
  };

  return (
    <section class="card" aria-labelledby="appearance-title">
      <h2 id="appearance-title">Aparência</h2>
      <fieldset class="mode-choice" aria-busy={saving()}>
        <legend>Tema</legend>
        <For each={THEMES}>
          {(option) => (
            <label>
              <input
                type="radio"
                name="theme"
                value={option.value}
                checked={(failures(), appearance().theme === option.value)}
                onChange={() => void choose({ ...appearance(), theme: option.value })}
              />
              {option.label}
            </label>
          )}
        </For>
      </fieldset>
      <fieldset class="mode-choice" aria-busy={saving()}>
        <legend>Tamanho do texto</legend>
        <For each={TEXT_SIZES}>
          {(option) => (
            <label>
              <input
                type="radio"
                name="text-size"
                value={option.value}
                checked={(failures(), appearance().textSize === option.value)}
                onChange={() => void choose({ ...appearance(), textSize: option.value })}
              />
              {option.label}
            </label>
          )}
        </For>
      </fieldset>
    </section>
  );
}
