import { useEffect } from "react";
import { X, Code2, Scale } from "lucide-react";
import logoUrl from "../assets/logo.png";

interface AboutDialogProps {
  open: boolean;
  onClose: () => void;
}

const REPO = "https://github.com/ECYCloud/node-speedtest";
const MIHOMO = "https://github.com/MetaCubeX/mihomo";

export default function AboutDialog({ open, onClose }: AboutDialogProps) {
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    document.addEventListener("keydown", onKey);
    const prev = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      document.removeEventListener("keydown", onKey);
      document.body.style.overflow = prev;
    };
  }, [open, onClose]);

  if (!open) return null;

  async function openExternal(url: string) {
    try {
      const { openUrl } = await import("@tauri-apps/plugin-opener");
      await openUrl(url);
    } catch {
      window.open(url, "_blank", "noopener,noreferrer");
    }
  }

  return (
    <div
      className="fixed inset-0 z-[100] flex items-center justify-center p-4 bg-black/50 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="w-full max-w-md rounded-2xl border border-border bg-bg-elev shadow-2xl overflow-hidden"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-3 px-5 pt-5 pb-3">
          <img
            src={logoUrl}
            alt=""
            className="w-12 h-12 rounded-xl object-contain shrink-0 select-none"
            draggable={false}
          />
          <div className="flex-1 min-w-0">
            <div className="text-lg font-semibold truncate">Node Speedtest</div>
            <div className="text-xs text-fg-muted">
              v{__APP_VERSION__} · 由 mihomo 内核驱动
            </div>
          </div>
          <button
            onClick={onClose}
            aria-label="关闭"
            className="w-8 h-8 inline-flex items-center justify-center rounded-full hover:bg-border/40 text-fg-muted shrink-0"
          >
            <X size={16} />
          </button>
        </div>

        <div className="px-5 pb-5 space-y-4 text-sm">
          <div className="rounded-xl border border-border bg-bg/50 p-3.5">
            <div className="flex items-center gap-2 text-xs font-medium text-fg-muted mb-1.5">
              <Scale size={13} />
              开源声明
            </div>
            <p className="text-[13px] leading-relaxed text-fg">
              本软件许可证为 MIT。桌面端为 Tauri + React，测速为进程内 Rust 异步引擎。
            </p>
            <p className="mt-1.5 text-[12px] leading-relaxed text-fg-muted">
              Copyright © 2026 ECYCloud · 见安装目录 LICENSE。
            </p>
            <p className="mt-1.5 text-[12px] leading-relaxed text-fg-muted">
              随附{" "}
              <button
                onClick={() => openExternal(MIHOMO)}
                className="text-primary hover:underline"
              >
                mihomo
              </button>
              {" "}为独立程序，沿用其原许可证（见 NOTICE）。
            </p>
          </div>

          <button
            onClick={() => openExternal(REPO)}
            className="w-full inline-flex items-center justify-center gap-1.5 h-9 rounded-full border border-border bg-bg hover:bg-border/30 text-xs font-medium transition"
          >
            <Code2 size={14} />
            项目仓库
          </button>
        </div>
      </div>
    </div>
  );
}
