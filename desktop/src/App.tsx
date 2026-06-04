import { useEffect, useState } from "react";
import { Activity, History, Settings, Sun, Moon, Sparkles } from "lucide-react";
import { useTheme } from "./store/theme";
import { startPolling } from "./store/test";
import { cn } from "./lib/cn";
import StatusPill from "./components/StatusPill";
import Speedtest from "./pages/Speedtest";
import HistoryPage from "./pages/History";
import SettingsPage from "./pages/SettingsPage";

type View = "speedtest" | "history" | "settings";

const NAV: { key: View; label: string; icon: React.ComponentType<{ size?: number }> }[] = [
  { key: "speedtest", label: "测速", icon: Activity },
  { key: "history", label: "历史记录", icon: History },
  { key: "settings", label: "设置", icon: Settings },
];

function ThemeToggle() {
  const { theme, toggle } = useTheme();
  const Icon = theme === "dark" ? Sun : Moon;
  return (
    <button
      onClick={toggle}
      title={theme === "dark" ? "切换到浅色" : "切换到深色"}
      className="inline-flex items-center justify-center w-9 h-9 rounded-lg border border-border bg-bg-elev hover:bg-border/50 active:scale-95 transition"
    >
      <Icon size={16} />
    </button>
  );
}

function Sidebar({ active, onChange }: { active: View; onChange: (v: View) => void }) {
  return (
    <aside className="flex flex-col w-56 shrink-0 border-r border-border bg-bg-elev/60 backdrop-blur-sm">
      <div className="px-5 py-4 flex items-center gap-2">
        <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-primary to-indigo-400 flex items-center justify-center text-white font-bold">
          S
        </div>
        <div className="leading-tight">
          <div className="font-semibold">Stair Speedtest</div>
          <div className="text-xs text-fg-muted">桌面版</div>
        </div>
      </div>
      <nav className="px-3 py-2 flex flex-col gap-1">
        {NAV.map(({ key, label, icon: Icon }) => (
          <button
            key={key}
            onClick={() => onChange(key)}
            className={cn(
              "flex items-center gap-3 px-3 py-2 rounded-lg text-sm",
              "hover:bg-border/40",
              active === key
                ? "bg-primary/10 text-primary font-medium"
                : "text-fg-muted hover:text-fg"
            )}
          >
            <Icon size={16} />
            {label}
          </button>
        ))}
      </nav>
      <div className="mt-auto px-3 py-3 text-xs text-fg-muted flex items-center gap-2">
        <Sparkles size={14} />
        <span>v0.1.0 · 桌面版</span>
      </div>
    </aside>
  );
}

function Topbar() {
  return (
    <header className="h-14 shrink-0 border-b border-border bg-bg-elev/40 backdrop-blur-sm flex items-center justify-between px-5">
      <div className="text-sm text-fg-muted">代理节点批量测速</div>
      <div className="flex items-center gap-2">
        <StatusPill />
        <ThemeToggle />
      </div>
    </header>
  );
}

export default function App() {
  const [view, setView] = useState<View>("speedtest");

  useEffect(() => {
    startPolling();
  }, []);

  return (
    <div className="h-full w-full flex bg-bg text-fg">
      <Sidebar active={view} onChange={setView} />
      <div className="flex-1 flex flex-col min-w-0">
        <Topbar />
        <main className="flex-1 flex flex-col p-6 overflow-auto min-h-0">
          {view === "speedtest" && <Speedtest />}
          {view === "history" && <HistoryPage />}
          {view === "settings" && <SettingsPage />}
        </main>
      </div>
    </div>
  );
}
