import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { pollGithubReleaseCheck, type GithubReleaseInfo } from "./githubReleaseCheck";

// /checkupdate 接口返回结构(由 webgui_wrapper.cpp 序列化):
//   - local       本地内核版本(mihomo -v 读出)
//   - latest      GitHub /releases/latest 的 tag_name,失败时为空
//   - has_update  latest > local 时为 true
//   - release_url 始终是 GitHub releases 页地址
//   - error       检查失败时的中文说明,成功时为空
export type MihomoUpdateInfo = GithubReleaseInfo;

export type MihomoInstallResult = {
  success: boolean;
  new_version: string;
  error: string;
};

// mihomo 内核更新状态 - 提到 zustand:
// 跟应用自更新一样,用户离开设置页(组件 unmount)后状态丢失会导致检查中按钮复位、
// 下载进度归零等差体验。统一搬到 store,跨页面切换无缝继续。
interface MihomoUpdateStore {
  checking: boolean;
  installing: boolean;
  info: MihomoUpdateInfo | null;
  installResult: MihomoInstallResult | null;
  check: () => Promise<void>;
  install: () => Promise<void>;
}

export const useMihomoUpdate = create<MihomoUpdateStore>((set, get) => ({
  checking: false,
  installing: false,
  info: null,
  installResult: null,

  async check() {
    if (get().checking) return;
    set({ checking: true, installResult: null });
    try {
      const info = await pollGithubReleaseCheck("/checkupdate");
      set({ info });
    } catch (e) {
      set({
        info: {
          local: "",
          latest: "",
          has_update: false,
          release_url: "https://github.com/MetaCubeX/mihomo/releases/latest",
          error: String(e),
        },
      });
    } finally {
      set({ checking: false });
    }
  },

  async install() {
    if (get().installing) return;
    set({ installing: true });
    try {
      // 后端会先 taskkill 在跑的 mihomo,再写入新 exe;失败自动从 .bak 还原。
      const r = await invoke<MihomoInstallResult>("download_mihomo_update");
      const cur = get().info;
      set({
        installResult: r,
        info: r.success && cur ? { ...cur, local: r.new_version, has_update: false } : cur,
      });
    } catch (e) {
      set({
        installResult: { success: false, new_version: "", error: String(e) },
      });
    } finally {
      set({ installing: false });
    }
  },
}));
