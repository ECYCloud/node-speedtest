import { invoke } from "@tauri-apps/api/core";

// 后端 /check*update 路由的统一返回结构(由 webgui_wrapper.cpp::serveUpdateCheck
// 序列化):mihomo 内核 (/checkupdate) 与软件自更新 (/checkappupdate) 同形,
// 差别只在 URL 与 release_url。
export type GithubReleaseInfo = {
  local: string;
  latest: string;
  has_update: boolean;
  release_url: string;
  error: string;
};

const PENDING_HINT = "正在检查...";
const POLL_INTERVAL_MS = 1500;
const POLL_TIMEOUT_MS = 20000;

// 后端首次访问 cache 未填充时回 error="正在检查...",这是进行中信号而非真错误,
// 前端轮询直到拿到真实结果(或超时,把 PENDING_HINT 当成空 error 一并返回)。
export async function pollGithubReleaseCheck(apiPath: string): Promise<GithubReleaseInfo> {
  const start = Date.now();
  for (;;) {
    const text = await invoke<string>("api_get", { path: apiPath });
    const info = JSON.parse(text) as GithubReleaseInfo;
    if (info.error === PENDING_HINT && Date.now() - start < POLL_TIMEOUT_MS) {
      await new Promise((r) => setTimeout(r, POLL_INTERVAL_MS));
      continue;
    }
    if (info.error === PENDING_HINT) info.error = "";
    return info;
  }
}
