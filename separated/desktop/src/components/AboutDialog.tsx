import { useEffect } from "react";
import { X, Code2, ExternalLink, Scale } from "lucide-react";
import logoUrl from "../assets/logo.png";

// 关于对话框:展示版本、二改来源(MIT)、mihomo 内核(GPL-3.0)、仓库链接。
// 法律层面安装向导已强制展示 MIT;此处是行业惯例的「关于」入口，方便用户随时查看。
interface AboutDialogProps {
  open: boolean;
  onClose: () => void;
}

const UPSTREAM = "https://github.com/tindy2013/stairspeedtest-reborn";
const REPO = "https://github.com/ECYCloud/stairspeedtest-reborn-mihomo";
const MIHOMO = "https://github.com/MetaCubeX/mihomo";

export default function AboutDialog({ open, onClose }: AboutDialogProps) {
  // ESC 关闭 + 锁住背景滚动，与系统对话框观感一致
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

  // 用 plugin-opener 走系统浏览器，纯 web 调试时回落到 window.open
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
        {/* 头部:logo + 应用名 + 关闭按钮 */}
        <div className="flex items-center gap-3 px-5 pt-5 pb-3">
          <img
            src={logoUrl}
            alt=""
            className="w-12 h-12 rounded-xl object-contain shrink-0 select-none"
            draggable={false}
          />
          <div className="flex-1 min-w-0">
            <div className="text-lg font-semibold truncate">Stair Speedtest</div>
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
          {/* 二改声明 */}
          <div className="rounded-xl border border-border bg-bg/50 p-3.5">
            <div className="flex items-center gap-2 text-xs font-medium text-fg-muted mb-1.5">
              <Scale size={13} />
              开源声明
            </div>
            <p className="text-[13px] leading-relaxed text-fg">
              本软件基于{" "}
              <button
                onClick={() => openExternal(UPSTREAM)}
                className="text-primary hover:underline"
              >
                tindy2013/stairspeedtest-reborn
              </button>
              {" "}二次开发，采用 MIT 许可证。
            </p>
            <p className="mt-1.5 text-[12px] leading-relaxed text-fg-muted">
              Copyright © 2019 tindy2013、© 2026 ECYCloud。
            </p>
            <p className="mt-1.5 text-[13px] leading-relaxed text-fg">
              内置{" "}
              <button
                onClick={() => openExternal(MIHOMO)}
                className="text-primary hover:underline"
              >
                MetaCubeX/mihomo
              </button>
              {" "}代理内核(GPL-3.0)，作为独立可执行文件随包分发。
            </p>
            <p className="mt-2 text-[11px] leading-relaxed text-fg-muted">
              完整许可证文本与源代码获取方式见安装目录下 LICENSE / NOTICE / licenses 文件。
            </p>
          </div>

          {/* 仓库链接 */}
          <div className="grid grid-cols-2 gap-2">
            <button
              onClick={() => openExternal(REPO)}
              className="inline-flex items-center justify-center gap-1.5 h-9 rounded-full border border-border bg-bg hover:bg-border/30 text-xs font-medium transition"
            >
              <Code2 size={14} />
              本仓库
            </button>
            <button
              onClick={() => openExternal(UPSTREAM)}
              className="inline-flex items-center justify-center gap-1.5 h-9 rounded-full border border-border bg-bg hover:bg-border/30 text-xs font-medium transition"
            >
              <ExternalLink size={14} />
              上游项目
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
