import { create } from "zustand";

export type Theme = "light" | "dark";

const STORAGE_KEY = "ssr-desktop-theme";

function getSystemTheme(): Theme {
  if (typeof window === "undefined") return "light";
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

function readInitialTheme(): Theme {
  if (typeof window === "undefined") return "light";
  const saved = window.localStorage.getItem(STORAGE_KEY) as Theme | null;
  return saved ?? getSystemTheme();
}

function applyTheme(theme: Theme) {
  if (typeof document === "undefined") return;
  const root = document.documentElement;
  root.classList.toggle("dark", theme === "dark");
  root.style.colorScheme = theme;
}

interface ThemeStore {
  theme: Theme;
  setTheme: (t: Theme) => void;
  toggle: () => void;
}

const initial = readInitialTheme();
applyTheme(initial);

export const useTheme = create<ThemeStore>((set, get) => ({
  theme: initial,
  setTheme: (t) => {
    applyTheme(t);
    window.localStorage.setItem(STORAGE_KEY, t);
    set({ theme: t });
  },
  toggle: () => {
    const next: Theme = get().theme === "dark" ? "light" : "dark";
    get().setTheme(next);
  },
}));
