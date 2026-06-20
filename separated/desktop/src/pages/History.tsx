import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Image as ImageIcon,
  FileText,
  RefreshCw,
  Trash2,
  Download,
  AlertCircle,
} from "lucide-react";
import { Badge, Button, Card, SectionTitle } from "../components/ui";
import { fmtBytes, fmtTime } from "../lib/format";
import { cn } from "../lib/cn";

interface HistoryItem {
  name: string;
  log_path: string | null;
  image_path: string | null;
  size: number;
  modified_ms: number;
}

export default function History() {
  const [items, setItems] = useState<HistoryItem[]>([]);
  const [active, setActive] = useState<HistoryItem | null>(null);
  const [imgData, setImgData] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [confirmClear, setConfirmClear] = useState(false);
  const [busy, setBusy] = useState(false);
  const [tip, setTip] = useState<string | null>(null);

  async function load() {
    setLoading(true);
    try {
      const list = await invoke<HistoryItem[]>("list_history");
      setItems(list);
      if (list.length > 0) {
        if (!active || !list.find((x) => x.name === active.name)) {
          setActive(list[0]);
        }
      } else {
        setActive(null);
      }
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    load();
  }, []);

  useEffect(() => {
    setImgData(null);
    if (!active?.image_path) return;
    invoke<string>("read_file_base64", { path: active.image_path }).then(
      (b64) => setImgData(`data:image/png;base64,${b64}`)
    );
  }, [active?.image_path]);

  async function deleteOne(it: HistoryItem) {
    setBusy(true);
    try {
      await invoke("delete_history_item", { name: it.name });
      setTip(`已删除 ${it.name}`);
      await load();
    } finally {
      setBusy(false);
      setTimeout(() => setTip(null), 2500);
    }
  }

  async function clearAll() {
    setBusy(true);
    try {
      const n = await invoke<number>("clear_history");
      setTip(`已清空 ${n} 个文件`);
      setConfirmClear(false);
      await load();
    } finally {
      setBusy(false);
      setTimeout(() => setTip(null), 2500);
    }
  }

  async function exportActive() {
    if (!active) return;
    // 标准"另存为"对话框:用户选保存位置与文件名(默认用记录名 + .png)。
    // 取所选路径的目录与基名，导出同名的 .png 与 .log 两个文件到该目录。
    const { save } = await import("@tauri-apps/plugin-dialog");
    const target = await save({
      title: "导出测速结果",
      defaultPath: `${active.name}.png`,
      filters: [{ name: "测速结果", extensions: ["png"] }],
    });
    if (!target || typeof target !== "string") return;
    // 拆出目录与基名(去扩展名)
    const norm = target.replace(/\\/g, "/");
    const slash = norm.lastIndexOf("/");
    const dir = slash >= 0 ? target.slice(0, slash) : "";
    let base = slash >= 0 ? norm.slice(slash + 1) : norm;
    base = base.replace(/\.(png|log)$/i, "");
    setBusy(true);
    try {
      const files = await invoke<string[]>("export_history", {
        name: active.name,
        targetDir: dir,
        destName: base,
      });
      setTip(`已导出 ${files.length} 个文件到 ${dir}`);
    } catch (e) {
      setTip(`导出失败: ${(e as Error).message}`);
    } finally {
      setBusy(false);
      setTimeout(() => setTip(null), 4000);
    }
  }

  return (
    <div className="grid grid-cols-1 lg:grid-cols-[340px_1fr] gap-4 min-h-0 flex-1">
      <Card className="p-5 flex flex-col min-h-0">
        <SectionTitle
          right={
            <div className="flex items-center gap-1.5">
              <Button
                size="sm"
                variant="ghost"
                onClick={load}
                disabled={loading || busy}
                title="刷新"
              >
                <RefreshCw
                  size={14}
                  className={loading ? "animate-spin" : ""}
                />
              </Button>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => setConfirmClear(true)}
                disabled={items.length === 0 || busy}
                title="清空全部"
              >
                <Trash2 size={14} />
              </Button>
            </div>
          }
          desc={`共 ${items.length} 条记录`}
        >
        </SectionTitle>
        {confirmClear && (
          <div className="mb-2 p-3 rounded-lg bg-danger/10 border border-danger/30">
            <div className="flex items-center gap-1.5 text-danger font-medium text-sm mb-2">
              <AlertCircle size={14} />
              确认清空全部历史?
            </div>
            <div className="flex gap-2">
              <Button
                size="sm"
                variant="danger"
                onClick={clearAll}
                disabled={busy}
              >
                确认清空
              </Button>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => setConfirmClear(false)}
                disabled={busy}
              >
                取消
              </Button>
            </div>
          </div>
        )}
        {tip && (
          <div className="mb-2 px-3 py-2 rounded-md bg-success/10 text-success text-xs">
            {tip}
          </div>
        )}
        <div className="overflow-auto flex flex-col gap-1.5 min-h-0">
          {items.map((it) => (
            <div
              key={it.name}
              className={cn(
                "group rounded-lg border transition",
                active?.name === it.name
                  ? "border-primary bg-primary/5"
                  : "border-border hover:bg-border/30"
              )}
            >
              <button
                onClick={() => setActive(it)}
                className="w-full text-left px-3 py-2.5"
              >
                <div className="text-sm font-medium truncate">{it.name}</div>
                <div className="text-xs text-fg-muted mt-1 flex items-center gap-2 tabular-nums">
                  <span>{fmtTime(it.modified_ms)}</span>
                  <span>·</span>
                  <span>{fmtBytes(it.size)}</span>
                </div>
                <div className="flex items-center gap-1 mt-1.5">
                  {it.image_path && (
                    <Badge variant="primary" className="gap-1">
                      <ImageIcon size={10} />
                      PNG
                    </Badge>
                  )}
                  {it.log_path && (
                    <Badge variant="neutral" className="gap-1">
                      <FileText size={10} />
                      LOG
                    </Badge>
                  )}
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      deleteOne(it);
                    }}
                    disabled={busy}
                    title="删除该条"
                    className="ml-auto p-1 rounded-full hover:bg-danger/15 text-fg-muted hover:text-danger opacity-0 group-hover:opacity-100 transition"
                  >
                    <Trash2 size={12} />
                  </button>
                </div>
              </button>
            </div>
          ))}
          {items.length === 0 && !loading && (
            <div className="text-fg-muted text-sm text-center py-8">
              暂无历史记录
            </div>
          )}
        </div>
      </Card>

      <Card className="p-5 flex flex-col min-h-0">
        <SectionTitle
          right={
            active && (
              <div className="flex items-center gap-1.5">
                <Button
                  size="sm"
                  onClick={exportActive}
                  disabled={busy}
                >
                  <Download size={14} />
                  导出
                </Button>
              </div>
            )
          }
          desc={active ? active.name : "选择左侧记录查看详情"}
        >
          结果详情
        </SectionTitle>
        {/* 居中容器换成左上对齐 + 自由滚动:旧版 flex items-center justify-center
            在原图比容器更大时会上下两端均匀裁切，体感"图片下方被吞了"。
            现在 overflow-auto 接管溢出，长图也能完整滚动看到。 */}
        <div className="flex-1 min-h-0 overflow-auto">
          {active && imgData ? (
            <img
              src={imgData}
              alt={active.name}
              className="block w-full h-auto rounded-lg shadow-sm border border-border"
            />
          ) : active ? (
            <div className="h-full flex items-center justify-center text-fg-muted text-sm">该记录没有图片</div>
          ) : (
            <div className="h-full flex items-center justify-center text-fg-muted text-sm">选择左侧记录查看详情</div>
          )}
        </div>
      </Card>
    </div>
  );
}
