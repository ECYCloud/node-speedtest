import { useEffect, useRef, useState } from "react";
import { CheckCircle2, AlertCircle, Loader2 } from "lucide-react";
import { api } from "../lib/api";
import { Badge } from "./ui";

type State = "checking" | "online" | "offline";

export default function StatusPill() {
  const [state, setState] = useState<State>("checking");
  const [version, setVersion] = useState<string>("");
  // ref 让 setTimeout 闭包永远读到最新 state——原版直接读 state 闭包变量,
  // 永远是初始 "checking",轮询间隔被锁在 2000ms。
  const stateRef = useRef<State>("checking");
  // 连续失败计数:单次抖动不立刻翻红,降低冷启动期间和后端 GC/重启时的闪红概率。
  const failCountRef = useRef(0);

  useEffect(() => {
    let alive = true;
    let timer: number;
    // sidecar 冷启动到 webserver 起来通常 1-3 秒,这窗口里 ping 必失败,
    // 强行翻红只会让用户看到 1-2 秒红色闪烁,体验差。
    const startedAt = Date.now();
    const GRACE_MS = 2000;

    async function ping() {
      try {
        const v = await api.version();
        if (!alive) return;
        failCountRef.current = 0;
        stateRef.current = "online";
        setState("online");
        setVersion(v.main);
      } catch {
        if (!alive) return;
        failCountRef.current += 1;
        // 启动宽限:启动后 GRACE_MS 内的失败统一显示为 checking
        if (Date.now() - startedAt < GRACE_MS) {
          stateRef.current = "checking";
          setState("checking");
        } else if (failCountRef.current >= 2) {
          stateRef.current = "offline";
          setState("offline");
        }
        // 单次失败:保留上一状态,等下一轮再判
      } finally {
        // online 时 8s 一轮足够;非 online 时 1.5s 频繁探,让恢复看起来"立刻"
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

  if (state === "checking")
    return (
      <Badge variant="neutral" className="gap-1.5">
        <Loader2 size={11} className="animate-spin" />
        正在连接后端
      </Badge>
    );
  if (state === "online")
    return (
      <Badge variant="success" className="gap-1.5">
        <CheckCircle2 size={11} />
        后端已就绪 {version && `· ${version}`}
      </Badge>
    );
  return (
    <Badge variant="danger" className="gap-1.5">
      <AlertCircle size={11} />
      后端未连接
    </Badge>
  );
}
