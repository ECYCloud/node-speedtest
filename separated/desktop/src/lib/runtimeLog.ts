import { invoke } from "@tauri-apps/api/core";

/** 追加一行到 engine/logs/runtime.log（「运行日志」页可见）。 */
export async function runtimeLog(
  message: string,
  level: "INFO" | "WARN" | "ERROR" = "INFO"
): Promise<void> {
  try {
    await invoke("append_runtime_log", { level, message });
  } catch {
    /* 写日志失败不阻断业务 */
  }
}
