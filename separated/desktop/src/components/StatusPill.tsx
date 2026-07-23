import { useEffect, useRef, useState } from "react";
import { AlertCircle, Loader2 } from "lucide-react";
import { api } from "../lib/api";
import { cn } from "../lib/cn";

type State = "checking" | "online" | "offline";

/** 顶栏引擎状态：在线时极简，避免高饱和色块抢视线。 */
export default function StatusPill() {
  const [state, setState] = useState<State>("checking");
  const stateRef = useRef<State>("checking");
  const failCountRef = useRef(0);

  useEffect(() => {
    let alive = true;
    let timer: number;
    const startedAt = Date.now();
    const GRACE_MS = 2000;

    async function ping() {
      try {
        await api.version();
        if (!alive) return;
        failCountRef.current = 0;
        stateRef.current = "online";
        setState("online");
      } catch {
        if (!alive) return;
        failCountRef.current += 1;
        if (Date.now() - startedAt < GRACE_MS) {
          stateRef.current = "checking";
          setState("checking");
        } else if (failCountRef.current >= 2) {
          stateRef.current = "offline";
          setState("offline");
        }
      } finally {
        const next = stateRef.current === "online" ? 8000 : 1500;
        timer = window.setTimeout(ping, next);
      }
    }
    ping();

    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, []);

  if (state === "checking") {
    return (
      <span
        className="inline-flex items-center gap-1.5 text-xs text-fg-muted"
        title="正在连接引擎"
      >
        <Loader2 size={12} className="animate-spin opacity-70" />
        连接中
      </span>
    );
  }

  if (state === "online") {
    return (
      <span
        className="inline-flex items-center gap-1.5 text-xs text-fg-muted"
        title="引擎已就绪"
      >
        <span
          className={cn("size-1.5 rounded-full bg-success/70")}
          aria-hidden
        />
        就绪
      </span>
    );
  }

  return (
    <span
      className="inline-flex items-center gap-1.5 text-xs text-danger/90"
      title="引擎未连接"
    >
      <AlertCircle size={12} />
      未连接
    </span>
  );
}
