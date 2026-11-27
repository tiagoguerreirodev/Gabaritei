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
  // The user's latest intended choice, until it is saved (or fails with nothing newer queued).
  const [desired, setDesired] = createSignal<Appearance | undefined>();
  const shown = () => desired() ?? appearance();

  // A choice made while a save is pending only updates `desired`; it is saved when the pending
  // save ends. If a save fails, a newer choice is still saved (as the user last intended it).
  const choose = async (change: Partial<Appearance>) => {
    setDesired({ ...shown(), ...change });
    if (saving()) return;
    setSaving(true);
    try {
      let target = desired();
      while (target) {
        try {
          await saveAppearance(target);
        } catch (error) {
          showError(error);
        }
        // Nothing newer: done, and the radios follow the appearance in effect again.
        if (desired() === target) setDesired(undefined);
        target = desired();
      }
    } finally {
      setSaving(false);
    }
  };

  return (
    <section class="card" aria-labelledby="appearance-title">
      <h2 id="appearance-title">Aparência</h2>
      <fieldset class="choice-group" aria-busy={saving()}>
        <legend>Tema</legend>
        <For each={THEMES}>
          {(option) => (
            <label>
              <input
                type="radio"
                name="theme"
                value={option.value}
                checked={shown().theme === option.value}
                onChange={() => void choose({ theme: option.value })}
              />
              {option.label}
            </label>
          )}
        </For>
      </fieldset>
      <fieldset class="choice-group" aria-busy={saving()}>
        <legend>Tamanho do texto</legend>
        <For each={TEXT_SIZES}>
          {(option) => (
            <label>
              <input
                type="radio"
                name="text-size"
                value={option.value}
                checked={shown().textSize === option.value}
                onChange={() => void choose({ textSize: option.value })}
              />
              {option.label}
            </label>
          )}
        </For>
      </fieldset>
    </section>
  );
}
