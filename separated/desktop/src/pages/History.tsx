import { useEffect, useRef, useState } from "react";
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
import { withRefreshSpin } from "../lib/refresh";
import { runtimeLog } from "../lib/runtimeLog";
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
  const [tipError, setTipError] = useState(false);
  const tipTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  function showTip(msg: string, isError = false) {
    setTip(msg);
    setTipError(isError);
    if (tipTimer.current) clearTimeout(tipTimer.current);
    tipTimer.current = setTimeout(() => setTip(null), isError ? 5000 : 2500);
  }

  useEffect(() => {
    return () => {
      if (tipTimer.current) clearTimeout(tipTimer.current);
    };
  }, []);

  const loadSeq = useRef(0);

  /** spin/log 仅手动点刷新时启用；进页静默加载，避免刷新钮空转与刷运行日志。 */
  async function load(opts?: { spin?: boolean; log?: boolean }) {
    const spin = opts?.spin === true;
    const log = opts?.log === true;
    const seq = ++loadSeq.current;
    const work = async () => {
      try {
        const list = await invoke<HistoryItem[]>("list_history");
        if (seq !== loadSeq.current) return;
        const next = Array.isArray(list) ? list : [];
        setItems(next);
        if (next.length > 0) {
          setActive((prev) => {
            if (prev && next.find((x) => x.name === prev.name)) return prev;
            return next[0];
          });
        } else {
          setActive(null);
        }
        if (log) {
          void runtimeLog(`历史记录：手动刷新，共 ${next.length} 条`);
        }
      } catch (e) {
        if (seq !== loadSeq.current) return;
        setItems([]);
        setActive(null);
        showTip(`刷新失败: ${e}`, true);
        if (log) {
          void runtimeLog(`历史记录：手动刷新失败 — ${e}`, "ERROR");
        }
      }
    };
    if (spin) await withRefreshSpin(setLoading, work);
    else await work();
  }

  useEffect(() => {
    void load();
  }, []);

  useEffect(() => {
    setImgData(null);
    if (!active?.image_path) return;
    let cancelled = false;
    const path = active.image_path;
    invoke<string>("read_file_base64", { path })
      .then((b64) => {
        if (!cancelled) setImgData(`data:image/png;base64,${b64}`);
      })
      .catch((e) => {
        if (!cancelled) showTip(`读图失败: ${e}`, true);
      });
    return () => {
      cancelled = true;
    };
  }, [active?.image_path]);

  async function deleteOne(it: HistoryItem) {
    setBusy(true);
    try {
      await invoke("delete_history_item", { name: it.name });
      showTip(`已删除 ${it.name}`);
      await load();
    } catch (e) {
      showTip(`删除失败: ${e}`, true);
    } finally {
      setBusy(false);
    }
  }

  async function clearAll() {
    setBusy(true);
    try {
      const n = await invoke<number>("clear_history");
      showTip(`已清空 ${n} 个文件`);
      setConfirmClear(false);
      await load();
    } catch (e) {
      showTip(`清空失败: ${e}`, true);
    } finally {
      setBusy(false);
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
      showTip(`已导出 ${files.length} 个文件到 ${dir}`);
    } catch (e) {
      showTip(`导出失败: ${e}`, true);
    } finally {
      setBusy(false);
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
                onClick={() => void load({ spin: true, log: true })}
                disabled={loading || busy}
                title="刷新"
              >
                <RefreshCw
                  size={14}
                  className={loading ? "animate-spin" : undefined}
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
          <div
            className={cn(
              "mb-2 px-3 py-2 rounded-md text-xs",
              tipError ? "bg-danger/10 text-danger" : "bg-success/10 text-success"
            )}
          >
            {tip}
          </div>
        )}
        <div className="overflow-auto flex flex-col gap-1.5 min-h-0">
          {items.map((it) => (
            <div
              key={it.name}
              role="button"
              tabIndex={0}
              onClick={() => setActive(it)}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  setActive(it);
                }
              }}
              className={cn(
                "group rounded-lg border transition cursor-pointer",
                active?.name === it.name
                  ? "border-primary bg-primary/5"
                  : "border-border hover:bg-border/30"
              )}
            >
              <div className="w-full text-left px-3 py-2.5">
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
                    type="button"
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
              </div>
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
              <Button size="sm" onClick={exportActive} disabled={busy}>
                <Download size={14} />
                导出原图
              </Button>
            )
          }
          desc={
            active
              ? `${active.name} · 适应宽度预览，导出仍为原图像素`
              : "选择左侧记录查看详情"
          }
        >
          结果详情
        </SectionTitle>
        <div className="flex-1 min-h-0 overflow-auto rounded-xl bg-bg border border-border p-4">
          {active && imgData ? (
            <div className="min-h-full flex justify-center">
              <img
                src={imgData}
                alt={active.name}
                draggable={false}
                className="rounded-lg shadow-sm border border-border bg-bg-elev max-w-full h-auto object-contain"
              />
            </div>
          ) : active ? (
            <div className="h-full flex items-center justify-center text-fg-muted text-sm">
              该记录没有图片
            </div>
          ) : (
            <div className="h-full flex items-center justify-center text-fg-muted text-sm">
              选择左侧记录查看详情
            </div>
          )}
        </div>
      </Card>
    </div>
  );
}
