import type { ITheme } from "@xterm/xterm";

// Single Sebenza theme, matching the gts-agency-web palette (MUI dark:
// near-black base, deep-blue chrome, cyan accent, amber/green/red semantics).
// The key stays "github-dark" so persisted values + defaults keep resolving.
export const THEME_KEYS = ["github-dark"] as const;
export type ThemeKey = (typeof THEME_KEYS)[number];

export interface ThemeDefinition {
  key: ThemeKey;
  label: string;
  colors: {
    surface: string;
    sidebar: string;
    topbar: string;
    hover: string;
    active: string;
    edge: string;
    primary: string;
    muted: string;
    accent: string;
    danger: string;
    success: string;
    warning: string;
  };
  terminal: ITheme;
}

export const THEMES: ThemeDefinition[] = [
  {
    key: "github-dark",
    label: "Sebenza",
    colors: {
      surface: "#0a0a0f",
      sidebar: "#0d1626",
      topbar: "#101d33",
      hover: "#17294a",
      active: "#00d4ff26",
      edge: "#20375a",
      primary: "#ffffff",
      muted: "#b0bec5",
      accent: "#00d4ff",
      danger: "#ff5252",
      success: "#00e676",
      warning: "#ffb74d",
    },
    terminal: {
      background: "#0a0a0f",
      foreground: "#e8eef4",
      cursor: "#00d4ff",
      selectionBackground: "#1e3a5f",
    },
  },
];

export function getTheme(key: string): ThemeDefinition {
  return THEMES.find((t) => t.key === key) ?? THEMES[0];
}
