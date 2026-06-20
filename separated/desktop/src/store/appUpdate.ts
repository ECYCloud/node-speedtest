import { create } from "zustand";
import { check as checkAppUpdater, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { getVersion } from "@tauri-apps/api/app";
import { pollGithubReleaseCheck } from "./githubReleaseCheck";

// 应用自更新状态机 - 由 tauri-plugin-updater 驱动。
// 状态提到 zustand store:用户离开设置页(组件 unmount)后,
// 后台 download 仍能继续推进进度，切回设置页能无缝看到当前状态。
//
// none 分支的 latest 由后端 /checkappupdate 提供(直接打 GitHub
// /releases/latest),目的是跟 mihomo 内核更新区视觉对齐——已是最新时也能展示
// "本地 X · 最新 X"。GitHub 不可达时 latest 为空串,UI 退化为只显示本地版本。
export type AppUpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "none"; current: string; latest: string }
  | { kind: "available"; current: string; latest: string; notes: string; update: Update }
  | { kind: "downloading"; current: string; latest: string; received: number; total: number }
  | { kind: "installing"; latest: string }
  | { kind: "ready"; latest: string }
  | { kind: "error"; message: string };

interface AppUpdateStore {
  state: AppUpdateState;
  check: () => Promise<void>;
  install: () => Promise<void>;
  reset: () => void;
}

export const useAppUpdate = create<AppUpdateStore>((set, get) => ({
  state: { kind: "idle" },

  async check() {
    set({ state: { kind: "checking" } });
    try {
      // 并行:plugin-updater 决定能否一键安装(主路径),后端 /checkappupdate 拿
      // GitHub 最新 tag(展示用)。GitHub 失败兜底空串，不影响主路径。
      // GitHub tag_name 形如 "v0.7.2",而 plugin-updater 返回的 update.version
      // 通常不带 v 前缀。前端在渲染时会统一拼 "v{latest}",这里把 ghLatest 去掉
      // 前导 v,与 plugin-updater 对齐，避免 "vv0.7.2" 重复 v。
      const [ghLatest, update] = await Promise.all([
        pollGithubReleaseCheck("/checkappupdate")
          .then((info) => (info.error ? "" : info.latest.replace(/^v/i, "")))
          .catch(() => ""),
        checkAppUpdater(),
      ]);
      // 守卫:await 期间用户离开设置页触发 reset,state 被设回 idle。
      // 此时不能再回写终态,否则下次进入设置页会看到"已是最新"等陈旧结果,
      // 重现用户抱怨的"没点检查就显示了"的问题。
      if (get().state.kind !== "checking") return;
      if (!update) {
        const current = await getVersion();
        if (get().state.kind !== "checking") return;
        set({ state: { kind: "none", current, latest: ghLatest } });
        return;
      }
      set({
        state: {
          kind: "available",
          current: update.currentVersion,
          latest: update.version,
          notes: update.body ?? "",
          update,
        },
      });
    } catch (e) {
      if (get().state.kind !== "checking") return;
      set({ state: { kind: "error", message: String(e) } });
    }
  },

  async install() {
    const cur = get().state;
    if (cur.kind !== "available") return;
    const u = cur.update;
    const latest = cur.latest;
    const current = cur.current;
    let received = 0;
    let total = 0;
    set({
      state: { kind: "downloading", current, latest, received: 0, total: 0 },
    });
    try {
      await u.downloadAndInstall((event) => {
        // 进度回调:reset 后 state 被设回 idle,继续推进度会把 UI 又拉回
        // downloading,违背用户"重置就是重置"的预期。下载本身没法取消,但至少
        // UI 不再回写 —— 用户回到设置页只看到干净的初始态。
        const k = get().state.kind;
        if (k !== "downloading" && k !== "installing") return;
        switch (event.event) {
          case "Started":
            total = event.data.contentLength ?? 0;
            set({
              state: { kind: "downloading", current, latest, received: 0, total },
            });
            break;
          case "Progress":
            received += event.data.chunkLength;
            set({
              state: { kind: "downloading", current, latest, received, total },
            });
            break;
          case "Finished":
            set({ state: { kind: "installing", latest } });
            break;
        }
      });
      if (get().state.kind !== "installing") return;
      set({ state: { kind: "ready", latest } });
      // Windows passive NSIS 自己接管;以下 relaunch 主要给 macOS/Linux 用
      await relaunch();
    } catch (e) {
      const k = get().state.kind;
      if (k !== "downloading" && k !== "installing") return;
      set({ state: { kind: "error", message: String(e) } });
    }
  },

  // 离开设置页时调用:状态完全重置回初始 idle。下次进入设置页是干净的初始
  // 界面,只有"检查更新"按钮,用户主动点击才进入 checking。
  // check/install 的 async 流程通过 state.kind 守卫识别"已被 reset",在 await
  // 完成后不再回写状态,确保陈旧结果不会"飞回"污染下一次访问。
  reset() {
    set({ state: { kind: "idle" } });
  },
}));
