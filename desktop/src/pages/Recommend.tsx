import { useState } from "react";
import { ExternalLink, Sparkles, ShieldCheck, Zap, Globe2 } from "lucide-react";
import { Badge, Card, SectionTitle, Button } from "../components/ui";
import { cn } from "../lib/cn";

interface Recommend {
  name: string;
  desc: string;
  url: string;
  /** 关键词标签:协议覆盖、节点数、流量套餐定位 */
  tags: string[];
  /** 高亮卖点 */
  highlights: { icon: typeof Sparkles; text: string }[];
}

// 推荐位仅供参考,不与机场存在任何商业绑定。所有点击通过系统浏览器打开,
// 不会在 webview 内嵌套加载第三方页面。后续如需调整,直接改这个数组。
const RECOMMENDS: Recommend[] = [
  {
    name: "示例机场 A",
    desc: "全协议支持(VLESS Reality / Hysteria2 / AnyTLS),IPLC 专线优化",
    url: "https://example.com/a",
    tags: ["VLESS", "Reality", "Hysteria2", "IPLC"],
    highlights: [
      { icon: Zap, text: "高带宽专线,适合 4K 流媒体" },
      { icon: ShieldCheck, text: "TLS 1.3 + Reality 抗审查" },
      { icon: Globe2, text: "覆盖 12 个地区 60+ 节点" },
    ],
  },
  {
    name: "示例机场 B",
    desc: "性价比首选,按月套餐起步流量充足",
    url: "https://example.com/b",
    tags: ["Trojan", "VMess", "WS+TLS"],
    highlights: [
      { icon: Zap, text: "晚高峰稳定 100Mbps+" },
      { icon: Globe2, text: "美国 / 日本 / 香港 三地优选" },
    ],
  },
];

export default function Recommend() {
  const [busyUrl, setBusyUrl] = useState<string | null>(null);

  // 通过 tauri-plugin-opener 在系统默认浏览器打开,不在 webview 内嵌任何外部页面。
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
        <SectionTitle desc="精选适合 Stair Speedtest 测速的机场;点击通过系统浏览器打开">
          机场推荐
        </SectionTitle>
        <div className="text-xs text-fg-muted">
          以下信息仅供参考,本软件不与任何机场存在分成或绑定。请自行核实订阅有效性与服务条款。
        </div>
      </Card>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        {RECOMMENDS.map((r) => (
          <Card key={r.name} className="p-5 flex flex-col gap-3">
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0">
                <div className="text-base font-semibold truncate">{r.name}</div>
                <div className="text-xs text-fg-muted mt-1 line-clamp-2">{r.desc}</div>
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
