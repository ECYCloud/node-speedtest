import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { RefreshCw, ScrollText, FileText } from "lucide-react";
import { Badge, Button, Card, SectionTitle } from "../components/ui";
import { fmtBytes, fmtTime } from "../lib/format";
import { withRefreshSpin } from "../lib/refresh";
import { runtimeLog } from "../lib/runtimeLog";
import { cn } from "../lib/cn";

interface LogFile {
  name: string;
  path: string;
  size: number;
  modified_ms: number;
}

// 给日志文件起个易懂的中文标签;未匹配的(测速主日志按时间戳命名)原样显示。
// 注:stderr 流承载的是后端运行期诊断信息(进度/警告/报错混合)，并非纯错误，
// 故标为"诊断输出"而非"错误输出"，避免内容为空时被误解为"漏了错误日志"。
function labelFor(name: string): string {
  if (name === "runtime.log") return "运行日志";
  if (name === "app-startup.log") return "启动日志";
  if (name === "sidecar-stderr.log") return "后端诊断输出";
  if (name === "sidecar-stdout.log") return "后端标准输出";
  return name.replace(/\.log$/, "");
}

// 单行着色:含 ERROR/WARN 标红/橙，其余默认。日志同时含正常与错误信息。
function lineClass(line: string): string {
  if (/\[ERROR\]|\berror\b|失败/i.test(line)) return "text-danger";
  if (/\[WARN(ING)?\]|\bwarning\b/i.test(line)) return "text-warning";
  return "";
}

export default function Logs() {
  const [files, setFiles] = useState<LogFile[]>([]);
  const [active, setActive] = useState<LogFile | null>(null);
  const [content, setContent] = useState("");
  const [loading, setLoading] = useState(false);
  const [tip, setTip] = useState<string | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const tipTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  function showTip(msg: string) {
    setTip(msg);
    if (tipTimer.current) clearTimeout(tipTimer.current);
    tipTimer.current = setTimeout(() => setTip(null), 5000);
  }

  useEffect(() => {
    return () => {
      if (tipTimer.current) clearTimeout(tipTimer.current);
    };
  }, []);

  const loadSeq = useRef(0);

  /** spin/log 仅手动点刷新时启用；进页静默加载。 */
  async function loadList(opts?: { spin?: boolean; log?: boolean }) {
    const spin = opts?.spin === true;
    const log = opts?.log === true;
    const seq = ++loadSeq.current;
    const work = async () => {
      try {
        const list = await invoke<LogFile[]>("list_log_files");
        if (seq !== loadSeq.current) return;
        const next = Array.isArray(list) ? list : [];
        setFiles(next);
        if (next.length > 0) {
          setActive((prev) => {
            if (prev && next.find((x) => x.name === prev.name)) return prev;
            return next[0];
          });
        } else {
          setActive(null);
          setContent("");
        }
        if (log) {
          void runtimeLog(`运行日志：手动刷新文件列表，共 ${next.length} 个`);
        }
      } catch (e) {
        if (seq !== loadSeq.current) return;
        setFiles([]);
        setActive(null);
        setContent("");
        showTip(`刷新失败: ${e}`);
        if (log) {
          void runtimeLog(`运行日志：手动刷新失败 — ${e}`, "ERROR");
        }
      }
    };
    if (spin) await withRefreshSpin(setLoading, work);
    else await work();
  }

  useEffect(() => {
    void loadList();
  }, []);

  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    const path = active.path;
    (async () => {
      try {
        const text = await invoke<string>("read_log_text", { path, maxKb: 512 });
        if (cancelled) return;
        setContent(text);
        requestAnimationFrame(() =>
          bottomRef.current?.scrollIntoView({ block: "end" })
        );
      } catch (e) {
        if (!cancelled) setContent(`(读取失败: ${e})`);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [active?.path]);

  return (
    <div className="grid grid-cols-1 lg:grid-cols-[340px_1fr] gap-4">
      <Card className="p-5 flex flex-col">
        <SectionTitle
          right={
            <Button
              size="sm"
              variant="ghost"
              onClick={() => void loadList({ spin: true, log: true })}
              disabled={loading}
              title="刷新"
            >
              <RefreshCw size={14} className={loading ? "animate-spin" : undefined} />
            </Button>
          }
          desc={`共 ${files.length} 个日志文件`}
        >
        </SectionTitle>
        {tip && (
          <div className="mb-2 px-3 py-2 rounded-md bg-danger/10 text-danger text-xs">
            {tip}
          </div>
        )}
        <div className="overflow-auto flex flex-col gap-1.5 max-h-[520px]">
          {files.map((f) => (
            <button
              key={f.name}
              onClick={() => setActive(f)}
              className={cn(
                "text-left px-3 py-2.5 rounded-lg border transition",
                active?.name === f.name
                  ? "border-primary bg-primary/5"
                  : "border-border hover:bg-border/30"
              )}
            >
              <div className="flex items-center gap-2 text-sm font-medium truncate">
                <FileText size={13} className="shrink-0 text-fg-muted" />
                {labelFor(f.name)}
              </div>
              <div className="text-xs text-fg-muted mt-1 flex items-center gap-2 tabular-nums">
                <span>{fmtTime(f.modified_ms)}</span>
                <span>·</span>
                <span>{fmtBytes(f.size)}</span>
              </div>
            </button>
          ))}
          {files.length === 0 && !loading && (
            <div className="text-fg-muted text-sm text-center py-8">暂无日志</div>
          )}
        </div>
      </Card>

      <Card className="p-5 flex flex-col min-w-0">
        <SectionTitle
          right={
            active && <Badge variant="neutral">{fmtBytes(active.size)}</Badge>
          }
          desc={active ? "仅显示最新 512 KB · 红色为错误，橙色为警告" : "选择左侧日志查看内容"}
        >
          <span className="inline-flex items-center gap-2">
            <ScrollText size={16} />
            {active ? labelFor(active.name) : "日志内容"}
          </span>
        </SectionTitle>
        <div className="overflow-auto max-h-[600px] rounded-lg border border-border bg-bg p-3">
          {content ? (
            <pre className="text-xs leading-relaxed whitespace-pre-wrap break-all font-mono">
              {content.split("\n").map((line, i) => (
                <div key={i} className={lineClass(line)}>{line || "\u00A0"}</div>
              ))}
              <div ref={bottomRef} />
            </pre>
          ) : (
            <div className="text-fg-muted text-sm text-center py-10">
              {active ? "(空日志)" : "选择左侧日志查看内容"}
            </div>
          )}
        </div>
      </Card>
    </div>
  );
}
