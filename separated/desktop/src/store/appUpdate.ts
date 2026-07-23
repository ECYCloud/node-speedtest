import { create } from "zustand";
import { check as checkAppUpdater, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { getVersion } from "@tauri-apps/api/app";
import { pollGithubReleaseCheck } from "./githubReleaseCheck";
import { runtimeLog } from "../lib/runtimeLog";

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

function stripV(v: string): string {
  return v.replace(/^v/i, "").trim();
}

function compareSemver(a: string, b: string): number {
  const pa = stripV(a).split(/[^0-9]+/).filter(Boolean).map((x) => parseInt(x, 10) || 0);
  const pb = stripV(b).split(/[^0-9]+/).filter(Boolean).map((x) => parseInt(x, 10) || 0);
  const n = Math.max(pa.length, pb.length);
  for (let i = 0; i < n; i++) {
    const x = pa[i] ?? 0;
    const y = pb[i] ?? 0;
    if (x > y) return 1;
    if (x < y) return -1;
  }
  return 0;
}

/** 把 reqwest/GitHub 网络错误翻成可操作的中文提示。 */
function friendlyNetError(err: unknown): string {
  const raw = String(err ?? "");
  const lower = raw.toLowerCase();
  if (
    lower.includes("error sending request") ||
    lower.includes("timed out") ||
    lower.includes("connection") ||
    lower.includes("dns") ||
    lower.includes("failed to lookup") ||
    lower.includes("network")
  ) {
    return "无法连接 GitHub 更新通道。请检查网络，或开启系统代理（Clash / mihomo 等）后重试；也可使用下方手动下载。";
  }
  return raw || "检查更新失败";
}

export const useAppUpdate = create<AppUpdateStore>((set, get) => ({
  state: { kind: "idle" },

  async check() {
    set({ state: { kind: "checking" } });
    void runtimeLog("设置：检查软件更新");
    try {
      // plugin-updater 失败不再拖垮整次检查：常见于国内直连 GitHub 超时。
      let update: Update | null = null;
      let updaterErr: unknown = null;
      try {
        update = await checkAppUpdater();
      } catch (e) {
        updaterErr = e;
      }

      let ghLatest = "";
      let ghHasUpdate = false;
      let ghError = "";
      try {
        const info = await pollGithubReleaseCheck("/checkappupdate");
        ghLatest = stripV(info.latest);
        ghHasUpdate = !!info.has_update;
        ghError = info.error || "";
      } catch {
        /* 展示用通道失败时忽略 */
      }

      if (get().state.kind !== "checking") return;

      if (update) {
        set({
          state: {
            kind: "available",
            current: update.currentVersion,
            latest: update.version,
            notes: update.body ?? "",
            update,
          },
        });
        void runtimeLog(
          `设置：发现软件新版本 v${update.version}（当前 v${update.currentVersion}）`
        );
        return;
      }

      const current = await getVersion();
      if (get().state.kind !== "checking") return;

      // 自动更新通道不可用，但 API 探测到新版本 → 引导手动下载
      if (updaterErr && (ghHasUpdate || (ghLatest && compareSemver(ghLatest, current) > 0))) {
        const message = `检测到新版本 v${ghLatest || "?"}，但自动更新通道暂时不可用。${friendlyNetError(updaterErr)}`;
        set({ state: { kind: "error", message } });
        void runtimeLog(`设置：软件更新检查异常 — ${message}`, "WARN");
        return;
      }

      // 两个通道都连不上
      if (updaterErr && !ghLatest) {
        const message = friendlyNetError(updaterErr || ghError);
        set({ state: { kind: "error", message } });
        void runtimeLog(`设置：软件更新检查失败 — ${message}`, "ERROR");
        return;
      }

      set({
        state: {
          kind: "none",
          current,
          latest: ghLatest || current,
        },
      });
      void runtimeLog(`设置：软件已是最新版本 v${ghLatest || current}`);
    } catch (e) {
      if (get().state.kind !== "checking") return;
      const message = friendlyNetError(e);
      set({ state: { kind: "error", message } });
      void runtimeLog(`设置：软件更新检查失败 — ${message}`, "ERROR");
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
    void runtimeLog(`设置：开始下载并安装软件更新 v${latest}`);
    try {
      await u.downloadAndInstall((event) => {
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
            void runtimeLog(`设置：软件更新下载完成，正在安装 v${latest}`);
            break;
        }
      });
      if (get().state.kind !== "installing") return;
      set({ state: { kind: "ready", latest } });
      void runtimeLog(`设置：软件已安装 v${latest}，即将重启`);
      await relaunch();
    } catch (e) {
      const k = get().state.kind;
      if (k !== "downloading" && k !== "installing") return;
      const message = friendlyNetError(e);
      set({ state: { kind: "error", message } });
      void runtimeLog(`设置：软件更新安装失败 — ${message}`, "ERROR");
    }
  },

  reset() {
    // 进行中/已可安装态不可清：离开设置页也不能打断下载与进度回调
    const k = get().state.kind;
    if (
      k === "checking" ||
      k === "available" ||
      k === "downloading" ||
      k === "installing"
    ) {
      return;
    }
    set({ state: { kind: "idle" } });
  },
}));
