import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { pollGithubReleaseCheck, type GithubReleaseInfo } from "./githubReleaseCheck";
import { runtimeLog } from "../lib/runtimeLog";

// /checkupdate 接口返回结构(由 webgui_wrapper.cpp 序列化):
//   - local       本地内核版本(mihomo -v 读出)
//   - latest      GitHub /releases/latest 的 tag_name,失败时为空
//   - has_update  latest > local 时为 true
//   - release_url 始终是 GitHub releases 页地址
//   - error       检查失败时的中文说明，成功时为空
export type MihomoUpdateInfo = GithubReleaseInfo;

export type MihomoInstallResult = {
  success: boolean;
  new_version: string;
  error: string;
};

// mihomo 内核更新状态 - 提到 zustand:
// 跟应用自更新一样，用户离开设置页(组件 unmount)后状态丢失会导致检查中按钮复位、
// 下载进度归零等差体验。统一搬到 store,跨页面切换无缝继续。
interface MihomoUpdateStore {
  checking: boolean;
  installing: boolean;
  info: MihomoUpdateInfo | null;
  installResult: MihomoInstallResult | null;
  check: () => Promise<void>;
  install: () => Promise<void>;
  reset: () => void;
}

// 全局 epoch 计数器:每次 reset 自增,异步流程通过比对识别身份是否过期。
// store 是单例,token 也只要一个。
let resetToken = 0;

export const useMihomoUpdate = create<MihomoUpdateStore>((set, get) => ({
  checking: false,
  installing: false,
  info: null,
  installResult: null,

  async check() {
    if (get().checking) return;
    // resetToken 在 reset() 时自增。check 启动时记录当前值，每次 await 后比对,
    // 不一致说明用户已离开设置页触发 reset,后续不再回写状态 —— 否则陈旧结果
    // 会"飞回"污染下次访问，重现用户抱怨的"没点检查就显示了已是最新"。
    const myToken = ++resetToken;
    set({ checking: true, installResult: null, info: null });
    void runtimeLog("设置：检查 mihomo 内核更新");
    try {
      const info = await pollGithubReleaseCheck("/checkupdate");
      if (myToken !== resetToken) return;
      set({ info });
      if (info.error) {
        void runtimeLog(`设置：mihomo 更新检查失败 — ${info.error}`, "ERROR");
      } else if (info.has_update) {
        void runtimeLog(
          `设置：发现 mihomo 新版本 ${info.latest}（本地 ${info.local || "未知"}）`
        );
      } else {
        void runtimeLog(
          `设置：mihomo 已是最新版本 ${info.latest || info.local || ""}`
        );
      }
    } catch (e) {
      if (myToken !== resetToken) return;
      set({
        info: {
          local: "",
          latest: "",
          has_update: false,
          release_url: "https://github.com/MetaCubeX/mihomo/releases/latest",
          error: String(e),
        },
      });
      void runtimeLog(`设置：mihomo 更新检查失败 — ${e}`, "ERROR");
    } finally {
      if (myToken === resetToken) set({ checking: false });
    }
  },

  async install() {
    if (get().installing) return;
    const myToken = ++resetToken;
    set({ installing: true });
    try {
      // 后端会先 taskkill 在跑的 mihomo,再写入新 exe;失败自动从 .bak 还原。
      const r = await invoke<MihomoInstallResult>("download_mihomo_update");
      if (myToken !== resetToken) return;
      const cur = get().info;
      set({
        installResult: r,
        info: r.success && cur ? { ...cur, local: r.new_version, has_update: false } : cur,
      });
      if (!r.success) {
        void runtimeLog(`设置：mihomo 安装失败 — ${r.error}`, "ERROR");
      }
    } catch (e) {
      if (myToken !== resetToken) return;
      set({
        installResult: { success: false, new_version: "", error: String(e) },
      });
      void runtimeLog(`设置：mihomo 安装失败 — ${e}`, "ERROR");
    } finally {
      if (myToken === resetToken) set({ installing: false });
    }
  },

  // 离开设置页时清掉终态展示(none/error)，避免下次进入先闪"已是最新"。
  // checking/installing 保留，并禁止自增 token，以免后台下载成功后无法回写。
  reset() {
    if (get().checking || get().installing) return;
    ++resetToken;
    set({ checking: false, installing: false, info: null, installResult: null });
  },
}));
