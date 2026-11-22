import type { JSX, ParentProps } from "solid-js";

/** Shared SVG frame: decorative (the visible or hidden text label names the control). */
function Icon(props: ParentProps) {
  return (
    <svg
      class="icon"
      viewBox="0 0 24 24"
      width="20"
      height="20"
      fill="none"
      stroke="currentColor"
      stroke-width="1.75"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      {props.children}
    </svg>
  );
}

export const HomeIcon = () => (
  <Icon>
    <path d="M4 11l8-7 8 7" />
    <path d="M6 10v9h12v-9" />
  </Icon>
);
export const BankIcon = () => (
  <Icon>
    <rect x="4" y="4" width="16" height="16" rx="2" />
    <path d="M4 10h16M10 10v10" />
  </Icon>
);
export const GenerateIcon = () => (
  <Icon>
    <path d="M12 4v4M12 16v4M4 12h4M16 12h4" />
    <path d="M7 7l2 2M15 15l2 2M17 7l-2 2M9 15l-2 2" />
  </Icon>
);
export const PackageIcon = () => (
  <Icon>
    <path d="M12 3l8 4v10l-8 4-8-4V7z" />
    <path d="M4 7l8 4 8-4M12 11v10" />
  </Icon>
);
export const SolveIcon = () => (
  <Icon>
    <circle cx="12" cy="12" r="8" />
    <path d="M8.5 12.5l2.5 2.5 4.5-5" />
  </Icon>
);
export const PerformanceIcon = () => (
  <Icon>
    <path d="M5 20V12M12 20V5M19 20v-5" />
  </Icon>
);
export const LearningIcon = () => (
  <Icon>
    <path d="M20 12a8 8 0 1 1-2.5-5.8" />
    <path d="M20 4v4h-4M12 8v4l3 2" />
  </Icon>
);
export const SettingsIcon = () => (
  <Icon>
    <path d="M5 7h14M5 17h14" />
    <circle cx="9" cy="7" r="2" fill="var(--surface)" />
    <circle cx="15" cy="17" r="2" fill="var(--surface)" />
  </Icon>
);
export const SidebarIcon = () => (
  <Icon>
    <rect x="4" y="5" width="16" height="14" rx="2" />
    <path d="M9 5v14" />
  </Icon>
);

export const menuIcons: Record<string, () => JSX.Element> = {
  "/": HomeIcon,
  "/banco": BankIcon,
  "/gerar": GenerateIcon,
  "/revisar-pacote": PackageIcon,
  "/resolver": SolveIcon,
  "/desempenho": PerformanceIcon,
  "/erros-e-revisoes": LearningIcon,
  "/configuracoes": SettingsIcon,
};
