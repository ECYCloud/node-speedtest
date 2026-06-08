import { useEffect, useState } from "react";
import { Activity, History, Settings, Sun, Moon, Sparkles, Compass, ScrollText } from "lucide-react";
import { useTheme } from "./store/theme";
import { startPolling } from "./store/test";
import { cn } from "./lib/cn";
import StatusPill from "./components/StatusPill";
import Speedtest from "./pages/Speedtest";
import HistoryPage from "./pages/History";
import SettingsPage from "./pages/SettingsPage";
import Recommend from "./pages/Recommend";
import Logs from "./pages/Logs";
// 侧栏品牌区图标(512x512 源，缩到小尺寸显示锐利)
import logoUrl from "./assets/logo.png";

type View = "speedtest" | "history" | "recommend" | "logs" | "settings";

const NAV: { key: View; label: string; icon: React.ComponentType<{ size?: number }> }[] = [
  { key: "speedtest", label: "节点测速", icon: Activity },
  { key: "history", label: "历史记录", icon: History },
  { key: "recommend", label: "机场推荐", icon: Compass },
  { key: "logs", label: "运行日志", icon: ScrollText },
  { key: "settings", label: "软件设置", icon: Settings },
];

function ThemeToggle() {
  const { theme, toggle } = useTheme();
  const Icon = theme === "dark" ? Sun : Moon;
  return (
    <button
      onClick={toggle}
      title={theme === "dark" ? "切换到浅色" : "切换到深色"}
      className="inline-flex items-center justify-center w-9 h-9 rounded-full border border-border bg-bg-elev hover:bg-border/50 active:scale-95 transition"
    >
      <Icon size={16} />
    </button>
  );
}

function Sidebar({ active, onChange }: { active: View; onChange: (v: View) => void }) {
  return (
    <aside className="flex flex-col w-56 shrink-0 border-r border-border bg-bg-elev/60 backdrop-blur-sm">
      {/* 品牌区:icon 在前、标题在后，始终同一行(Clash Verge Rev 风格)。
          高度与右侧 Topbar(h-14)一致，使左右两区文字处于同一水平线。 */}
      <div className="h-14 flex items-center gap-2.5 px-4 border-b border-border">
        <img
          src={logoUrl}
          alt="Stair Speedtest"
          className="w-9 h-9 rounded-lg object-contain shrink-0 select-none"
          draggable={false}
        />
        <span className="font-semibold text-base whitespace-nowrap truncate">
          Stair Speedtest
        </span>
      </div>
      <nav className="px-3 pt-3 pb-2 flex flex-col gap-1">
        {NAV.map(({ key, label, icon: Icon }) => (
          <button
            key={key}
            onClick={() => onChange(key)}
            className={cn(
              "flex items-center gap-3 px-4 py-2 rounded-full text-sm transition",
              "hover:bg-border/40",
              active === key
                ? "bg-primary text-primary-fg shadow-sm shadow-primary/30 font-medium hover:bg-primary"
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

function Topbar({ view }: { view: View }) {
  // 顶栏标题随当前界面变化，直接复用导航里的 label
  const title = NAV.find((n) => n.key === view)?.label ?? "";
  return (
    <header className="h-14 shrink-0 border-b border-border bg-bg-elev/40 backdrop-blur-sm flex items-center justify-between px-5">
      <div className="text-base font-semibold">{title}</div>
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
        <Topbar view={view} />
        <main className="flex-1 flex flex-col p-6 overflow-auto min-h-0">
          {view === "speedtest" && <Speedtest />}
          {view === "history" && <HistoryPage />}
          {view === "recommend" && <Recommend />}
          {view === "logs" && <Logs />}
          {view === "settings" && <SettingsPage />}
        </main>
      </div>
    </div>
  );
}
