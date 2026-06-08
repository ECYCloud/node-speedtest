import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { RefreshCw, ScrollText, FileText } from "lucide-react";
import { Badge, Button, Card, SectionTitle } from "../components/ui";
import { fmtBytes, fmtTime } from "../lib/format";
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
  if (name === "sidecar-stderr.log") return "后端诊断输出";
  if (name === "sidecar-stdout.log") return "后端标准输出";
  if (name === "app-startup.log") return "启动日志";
  return name.replace(/\.log$/, "");
}

// 单行着色:含 ERROR/WARN 标红/橙，其余默认。日志同时含正常与错误信息。
function lineClass(line: string): string {
  if (/\[ERROR\]|error|失败|ERROR/.test(line)) return "text-danger";
  if (/\[WARN(ING)?\]|warning/i.test(line)) return "text-warning";
  return "";
}

export default function Logs() {
  const [files, setFiles] = useState<LogFile[]>([]);
  const [active, setActive] = useState<LogFile | null>(null);
  const [content, setContent] = useState("");
  const [loading, setLoading] = useState(false);
  const bottomRef = useRef<HTMLDivElement>(null);

  async function loadList() {
    setLoading(true);
    try {
      const list = await invoke<LogFile[]>("list_log_files");
      setFiles(list);
      if (list.length > 0 && (!active || !list.find((x) => x.name === active.name))) {
        setActive(list[0]);
      } else if (list.length === 0) {
        setActive(null);
        setContent("");
      }
    } finally {
      setLoading(false);
    }
  }

  async function loadContent(f: LogFile) {
    const text = await invoke<string>("read_log_text", { path: f.path, maxKb: 512 });
    setContent(text);
    requestAnimationFrame(() =>
      bottomRef.current?.scrollIntoView({ block: "end" })
    );
  }

  useEffect(() => {
    loadList();
  }, []);

  useEffect(() => {
    if (active) loadContent(active);
  }, [active?.path]);

  return (
    <div className="grid grid-cols-1 lg:grid-cols-[340px_1fr] gap-4">
      <Card className="p-5 flex flex-col">
        <SectionTitle
          right={
            <Button size="sm" variant="ghost" onClick={loadList} disabled={loading} title="刷新">
              <RefreshCw size={14} className={loading ? "animate-spin" : ""} />
            </Button>
          }
          desc={`共 ${files.length} 个日志文件`}
        >
        </SectionTitle>
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
