import { useEffect, useState } from "react";
import { CheckCircle2, AlertCircle, Loader2 } from "lucide-react";
import { api } from "../lib/api";
import { Badge } from "./ui";

type State = "checking" | "online" | "offline";

export default function StatusPill() {
  const [state, setState] = useState<State>("checking");
  const [version, setVersion] = useState<string>("");

  useEffect(() => {
    let alive = true;
    let timer: number;

    async function ping() {
      try {
        const v = await api.version();
        if (!alive) return;
        setState("online");
        setVersion(v.main);
      } catch {
        if (!alive) return;
        setState("offline");
      } finally {
        timer = window.setTimeout(ping, state === "online" ? 8000 : 2000);
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
