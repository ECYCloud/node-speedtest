import { invoke } from "@tauri-apps/api/core";

/* 后端 HTTP 接口封装。所有请求经 Rust 侧 reqwest 代理(invoke),
   绕过 webview 在 release 模式下的 mixed-content / CORS preflight 限制。 */

async function apiGet<T>(path: string, parseJson = true): Promise<T> {
  const text = await invoke<string>("api_get", { path });
  return (parseJson ? safeJson<T>(path, text) : (text as T));
}

async function apiPostJson<T>(
  path: string,
  body: unknown,
  parseJson = true
): Promise<T> {
  const text = await invoke<string>("api_post_json", {
    path,
    body: JSON.stringify(body),
  });
  return (parseJson ? safeJson<T>(path, text) : (text as T));
}

async function apiPostFile<T>(
  path: string,
  fileName: string,
  fileBytes: number[]
): Promise<T> {
  const text = await invoke<string>("api_post_file", {
    path,
    fileName,
    fileBytes,
  });
  return safeJson<T>(path, text);
}

/** 后端在某些状态下会返回 "running" / "error" 这类纯文本而非 JSON,
    这里集中识别并抛出语义清晰的错误，避免污染上层 store 的数据状态。 */
function safeJson<T>(path: string, text: string): T {
  const trimmed = text.trim();
  if (trimmed === "running") {
    throw new Error("后端正在测速中，请等当前任务结束");
  }
  if (trimmed === "error") {
    throw new Error("后端无法识别该配置文件");
  }
  if (trimmed === "done") {
    throw new Error("上次测速刚刚结束，请稍候再试");
  }
  try {
    return JSON.parse(trimmed) as T;
  } catch {
    throw new Error(`${path} 返回格式异常: ${trimmed.slice(0, 80)}`);
  }
}

export interface VersionInfo {
  main: string;
  webapi: string;
}

export interface NodeConfig {
  type: string;
  config: {
    group: string;
    remarks: string;
    server: string;
    server_port: number;
  };
}

export interface GeoBlock {
  address: string;
  info: string;
}

export interface NodeResult {
  group: string;
  remarks: string;
  loss: number;
  ping: number;
  gPing: number;
  rawSocketSpeed: number[];
  rawTcpPingStatus: number[];
  rawGooglePingStatus: number[];
  gPingLoss: number;
  geoIP: { inbound: GeoBlock; outbound: GeoBlock };
  dspeed: number;
  /** 最高瞬时速度，字节/秒;后端 webgui_wrapper.json_write_node 写入。 */
  dspeedMax: number;
  trafficUsed: number;
}

export interface ResultsPayload {
  status: "running" | "stopped";
  current: Partial<NodeResult>;
  results: NodeResult[];
}

export interface StartParams {
  configs: NodeConfig[];
  testMode: "ALL" | "TCP_PING";
  sortMethod: string;
  group: string;
}

export const api = {
  version: () => apiGet<VersionInfo>("/getversion"),
  status: () => apiGet<string>("/status", false),
  readSubscription: (url: string) =>
    apiPostJson<NodeConfig[]>("/readsubscriptions", { url }),
  readFileConfig: (fileName: string, fileBytes: number[]) =>
    apiPostFile<NodeConfig[]>("/readfileconfig", fileName, fileBytes),
  start: (params: StartParams) =>
    apiPostJson<string>("/start", params, false),
  // 节点级停止:后端把 stop_requested 置 true,batchTest 当前节点跑完后跳出循环。
  // 不杀后端进程,allNodes/targetNodes 全程保留,再点开始可无缝继续。
  stop: () => apiPostJson<string>("/stop", {}, false),
  results: () => apiGet<ResultsPayload>("/getresults"),
};
