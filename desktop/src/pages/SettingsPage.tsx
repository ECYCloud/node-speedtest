import { Button, Card, SectionTitle } from "../components/ui";
import { RefreshCw, Monitor } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";

export default function SettingsPage() {
  const [restarting, setRestarting] = useState(false);

  async function restart() {
    setRestarting(true);
    try {
      await invoke("restart_backend");
    } finally {
      setRestarting(false);
    }
  }

  return (
    <div className="flex flex-col gap-4 max-w-3xl">
      <Card className="p-5">
        <SectionTitle desc="后端进程异常或修改 pref.ini 后，可手动重启">
          后端进程
        </SectionTitle>
        <Button onClick={restart} disabled={restarting}>
          <RefreshCw
            size={14}
            className={restarting ? "animate-spin" : ""}
          />
          重启后端
        </Button>
      </Card>

      <Card className="p-5">
        <SectionTitle desc="测速参数等高级设置直接编辑 engine/pref.ini">
          高级配置
        </SectionTitle>
        <div className="flex items-center gap-2 text-sm text-fg-muted">
          <Monitor size={14} />
          <code className="text-xs">engine/pref.ini</code>
          <span>·</span>
          <span>修改后点击上方"重启后端"生效</span>
        </div>
      </Card>
    </div>
  );
}
