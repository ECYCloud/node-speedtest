import { create } from "zustand";
import { check as checkAppUpdater, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { getVersion } from "@tauri-apps/api/app";

// 应用自更新状态机 - 由 tauri-plugin-updater 驱动。
// 状态提到 zustand store:用户离开设置页(组件 unmount)后,
// 后台 download 仍能继续推进进度,切回设置页能无缝看到当前状态。
export type AppUpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "none"; current: string }
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
      const update = await checkAppUpdater();
      if (!update) {
        const current = await getVersion();
        set({ state: { kind: "none", current } });
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
      set({ state: { kind: "ready", latest } });
      // Windows passive NSIS 自己接管;以下 relaunch 主要给 macOS/Linux 用
      await relaunch();
    } catch (e) {
      set({ state: { kind: "error", message: String(e) } });
    }
  },

  reset() {
    set({ state: { kind: "idle" } });
  },
}));
