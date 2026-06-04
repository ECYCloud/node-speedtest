import { useRef, useState } from "react";
import { Link2, FileUp, Loader2 } from "lucide-react";
import { useTest } from "../../store/test";
import { Button, Card, Input, SectionTitle } from "../../components/ui";

export default function Importer() {
  const [url, setUrl] = useState("");
  const [tab, setTab] = useState<"sub" | "file">("sub");
  const fileRef = useRef<HTMLInputElement>(null);
  const { loadSubscription, loadFileConfig, loadingConfigs, configs, error } =
    useTest();

  async function onLoadSub() {
    const u = url.trim();
    if (!u) return;
    await loadSubscription(u);
  }

  async function onPickFile(file: File) {
    const buf = await file.arrayBuffer();
    // Tauri invoke 不接受 ArrayBuffer/Uint8Array,序列化成普通数字数组
    const bytes = Array.from(new Uint8Array(buf));
    await loadFileConfig(file.name, bytes);
  }

  return (
    <Card className="p-5">
      <SectionTitle desc="支持订阅链接、本地配置文件(SSR / V2Ray / Trojan / Clash 等)">
        导入节点
      </SectionTitle>

      <div className="inline-flex items-center gap-1 p-1 rounded-lg bg-border/40 mb-4 text-sm">
        {(["sub", "file"] as const).map((k) => (
          <button
            key={k}
            onClick={() => setTab(k)}
            className={
              "px-3 h-7 rounded-md transition " +
              (tab === k
                ? "bg-bg-elev text-fg shadow-sm"
                : "text-fg-muted hover:text-fg")
            }
          >
            {k === "sub" ? "订阅链接" : "本地文件"}
          </button>
        ))}
      </div>

      {tab === "sub" ? (
        <div className="flex items-center gap-2">
          <Link2 size={16} className="text-fg-muted shrink-0" />
          <Input
            placeholder="粘贴订阅链接或单个节点链接"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && onLoadSub()}
            disabled={loadingConfigs}
            className="min-w-0 flex-1"
          />
          <Button
            variant="primary"
            disabled={loadingConfigs || !url.trim()}
            onClick={onLoadSub}
            className="shrink-0 whitespace-nowrap min-w-[5.5rem] justify-center"
          >
            {loadingConfigs ? (
              <>
                <Loader2 size={14} className="animate-spin" />
                读取中
              </>
            ) : (
              "读取"
            )}
          </Button>
        </div>
      ) : (
        <div className="flex items-center gap-2">
          <Button
            variant="secondary"
            disabled={loadingConfigs}
            onClick={() => fileRef.current?.click()}
          >
            <FileUp size={14} />
            选择文件
          </Button>
          <span className="text-xs text-fg-muted">
            支持 .json / .yaml / .conf / .txt 等
          </span>
          <input
            ref={fileRef}
            type="file"
            className="hidden"
            onChange={(e) => {
              const f = e.target.files?.[0];
              if (f) onPickFile(f);
              e.target.value = "";
            }}
          />
        </div>
      )}

      {error && (
        <div className="mt-3 text-xs text-danger">{error}</div>
      )}
      {!error && configs.length > 0 && (
        <div className="mt-3 text-xs text-fg-muted">
          已识别 {configs.length} 个节点
        </div>
      )}
    </Card>
  );
}
