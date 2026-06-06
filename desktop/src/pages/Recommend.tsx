import { useState } from "react";
import { ExternalLink, Sparkles, ShieldCheck, Zap, Globe2 } from "lucide-react";
import { Badge, Card, Button } from "../components/ui";
import { cn } from "../lib/cn";
// 把第三方机场的 favicon 打包到本地，运行时不发起任何外链请求(遵循 AGENTS.md 静态资源本地化原则)
import ecyFavicon from "../assets/ecy-favicon.ico";

interface Recommend {
  name: string;
  desc: string;
  url: string;
  /** 卡片左上角站点 favicon，缺省时不渲染 */
  icon?: string;
  /** 关键词标签:协议覆盖、节点数、流量套餐定位 */
  tags: string[];
  /** 高亮卖点 */
  highlights: { icon: typeof Sparkles; text: string }[];
}

// 推荐位仅供参考，不与机场存在任何商业绑定。所有点击通过系统浏览器打开，
// 不会在 webview 内嵌套加载第三方页面。后续如需调整，直接改这个数组。
const RECOMMENDS: Recommend[] = [
  {
    name: "ECY Cloud",
    desc: "",
    url: "https://owo.ecycloud.com",
    icon: ecyFavicon,
    tags: ["VLESS", "Hysteria2", "AnyTLS", "Trojan"],
    highlights: [
      { icon: Zap, text: "高带宽专线，适合 4K 流媒体" },
      { icon: ShieldCheck, text: "TLS 1.3 + Reality 抗审查" },
      { icon: Globe2, text: "覆盖 8 个地区 40+ 节点" },
    ],
  },
];

export default function Recommend() {
  const [busyUrl, setBusyUrl] = useState<string | null>(null);

  // 通过 tauri-plugin-opener 在系统默认浏览器打开，不在 webview 内嵌任何外部页面。
  async function openInBrowser(url: string) {
    setBusyUrl(url);
    try {
      const { openUrl } = await import("@tauri-apps/plugin-opener");
      await openUrl(url);
    } catch {
      // 非 Tauri 环境(纯 web 调试)走 window.open 兜底
      window.open(url, "_blank", "noopener,noreferrer");
    } finally {
      setBusyUrl(null);
    }
  }

  return (
    <div className="flex flex-col gap-4">
      <Card className="p-5">
        <div className="text-xs text-fg-muted">
          点击 访问 按钮通过浏览器打开
        </div>
      </Card>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        {RECOMMENDS.map((r) => (
          <Card key={r.name} className="p-5 flex flex-col gap-3">
            <div className="flex items-start justify-between gap-3">
              {/* 标题行:有描述时 icon 顶对齐(跨两行更稳)，无描述时 icon 与单行标题居中对齐 */}
              <div
                className={cn(
                  "min-w-0 flex gap-3",
                  r.desc ? "items-start" : "items-center"
                )}
              >
                {r.icon && (
                  <img
                    src={r.icon}
                    alt=""
                    aria-hidden
                    className="w-6 h-6 rounded-md object-contain shrink-0 select-none"
                    draggable={false}
                  />
                )}
                <div className="min-w-0">
                  <div className="text-base font-semibold truncate">{r.name}</div>
                  {r.desc && (
                    <div className="text-xs text-fg-muted mt-1 line-clamp-2">{r.desc}</div>
                  )}
                </div>
              </div>
              <Button
                size="sm"
                variant="primary"
                disabled={busyUrl === r.url}
                onClick={() => openInBrowser(r.url)}
                className="shrink-0"
              >
                <ExternalLink size={14} />
                访问
              </Button>
            </div>

            <div className="flex flex-wrap gap-1.5">
              {r.tags.map((t) => (
                <Badge key={t} variant="primary">{t}</Badge>
              ))}
            </div>

            <div className="flex flex-col gap-1.5 mt-1">
              {r.highlights.map(({ icon: Icon, text }, i) => (
                <div
                  key={i}
                  className={cn("flex items-center gap-2 text-xs text-fg-muted")}
                >
                  <Icon size={12} className="text-primary shrink-0" />
                  <span>{text}</span>
                </div>
              ))}
            </div>
          </Card>
        ))}
      </div>
    </div>
  );
}
