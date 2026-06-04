import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
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
    const dir = await openDialog({
      directory: true,
      multiple: false,
      title: "选择导出位置",
    });
    if (!dir || typeof dir !== "string") return;
    const prefix = window.prompt("输入分组前缀(将作为文件名前缀,可留空):", "") ?? "";
    setBusy(true);
    try {
      const files = await invoke<string[]>("export_history", {
        name: active.name,
        targetDir: dir,
        prefix,
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
      <Card className="p-4 flex flex-col min-h-0">
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
          历史记录
        </SectionTitle>
        {confirmClear && (
          <div className="mb-2 p-2.5 rounded-lg bg-danger/10 border border-danger/30 text-xs">
            <div className="flex items-center gap-1.5 text-danger font-medium mb-1.5">
              <AlertCircle size={12} />
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
          <div className="mb-2 px-2.5 py-1.5 rounded-md bg-success/10 text-success text-xs">
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
                <div className="text-xs text-fg-muted mt-1 flex items-center gap-2">
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
                    className="ml-auto p-1 rounded hover:bg-danger/15 text-fg-muted hover:text-danger opacity-0 group-hover:opacity-100 transition"
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
        <div className="flex-1 flex items-center justify-center min-h-0 overflow-auto">
          {active && imgData ? (
            <img
              src={imgData}
              alt={active.name}
              className="max-w-full max-h-full rounded-lg shadow-sm border border-border"
            />
          ) : active ? (
            <div className="text-fg-muted text-sm">该记录没有图片</div>
          ) : (
            <div className="text-fg-muted text-sm">选择左侧记录查看详情</div>
          )}
        </div>
      </Card>
    </div>
  );
}
