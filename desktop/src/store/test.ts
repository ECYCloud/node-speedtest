import { create } from "zustand";
import { api, NodeConfig, NodeResult, StartParams } from "../lib/api";

/** 后端在解析失败时可能返回 type=Unknown 且没有 config 字段的占位项,
    那种节点缺少 server/remarks 等关键信息,无法用于测速,直接丢弃。
    其他协议(SS/SSR/V2Ray/Trojan/Snell/VLESS/Hysteria2/AnyTLS/...)只要带完整 config 都保留。 */
function sanitizeConfigs(list: unknown): NodeConfig[] {
  if (!Array.isArray(list)) return [];
  return list.filter((x): x is NodeConfig => {
    if (!x || typeof x !== "object") return false;
    const node = x as Partial<NodeConfig>;
    if (typeof node.type !== "string") return false;
    // 没有 config 字段的(后端 Unknown 分支)直接丢,无法测速
    if (!node.config || typeof node.config !== "object") return false;
    if (typeof node.config.remarks !== "string") return false;
    return true;
  });
}

interface TestStore {
  status: "stopped" | "running";
  configs: NodeConfig[];
  selected: Set<number>;
  results: NodeResult[];
  current: Partial<NodeResult> | null;
  filter: string;
  typeFilter: Set<string>;
  loadingConfigs: boolean;
  error: string | null;
  /** 分组名:用于 PNG 标题、历史记录文件名前缀、统一所有节点的 group 字段。
      在导入节点时填写,留空则后端用第一个节点的协议默认 group。 */
  group: string;

  loadSubscription: (url: string) => Promise<void>;
  loadFileConfig: (fileName: string, fileBytes: number[]) => Promise<void>;
  toggleSelect: (i: number) => void;
  /** 全选:不传参表示选中全部 configs;传可见 indices 时只选这些 */
  selectAll: (indices?: number[]) => void;
  clearSelect: () => void;
  setFilter: (s: string) => void;
  toggleType: (t: string) => void;
  setGroup: (g: string) => void;
  startTest: (
    testMode: StartParams["testMode"],
    sortMethod: string,
    colors: string
  ) => Promise<void>;
  /** 恢复测试:基于当前 results 跳过已测节点,只测剩下的(未测 + 失败的) */
  resumeTest: (
    testMode: StartParams["testMode"],
    sortMethod: string,
    colors: string
  ) => Promise<void>;
  stopTest: () => Promise<void>;
  refreshOnce: () => Promise<void>;
}

export const useTest = create<TestStore>((set, get) => ({
  status: "stopped",
  configs: [],
  selected: new Set(),
  results: [],
  current: null,
  filter: "",
  typeFilter: new Set(),
  loadingConfigs: false,
  error: null,
  group: "",

  async loadSubscription(url) {
    set({ loadingConfigs: true, error: null });
    try {
      const list = await api.readSubscription(url);
      const safe = sanitizeConfigs(list);
      set({
        configs: safe,
        selected: new Set(safe.map((_, i) => i)),
        results: [],
        current: null,
      });
    } catch (e) {
      set({ error: (e as Error).message, configs: [], selected: new Set() });
    } finally {
      set({ loadingConfigs: false });
    }
  },

  async loadFileConfig(fileName, fileBytes) {
    set({ loadingConfigs: true, error: null });
    try {
      const list = await api.readFileConfig(fileName, fileBytes);
      const safe = sanitizeConfigs(list);
      set({
        configs: safe,
        selected: new Set(safe.map((_, i) => i)),
        results: [],
        current: null,
      });
    } catch (e) {
      set({ error: (e as Error).message, configs: [], selected: new Set() });
    } finally {
      set({ loadingConfigs: false });
    }
  },

  toggleSelect(i) {
    const s = new Set(get().selected);
    s.has(i) ? s.delete(i) : s.add(i);
    set({ selected: s });
  },
  selectAll(indices) {
    if (indices && indices.length >= 0) {
      set({ selected: new Set(indices) });
    } else {
      set({ selected: new Set(get().configs.map((_, i) => i)) });
    }
  },
  clearSelect() {
    set({ selected: new Set() });
  },
  setFilter(s) {
    set({ filter: s });
  },
  toggleType(t) {
    const s = new Set(get().typeFilter);
    s.has(t) ? s.delete(t) : s.add(t);
    set({ typeFilter: s });
  },
  setGroup(g) {
    set({ group: g });
  },

  async startTest(testMode, sortMethod, colors) {
    const { configs, selected, group } = get();
    const picked = [...selected]
      .sort((a, b) => a - b)
      .map((i) => configs[i])
      .filter(Boolean);
    if (!picked.length) return;
    set({ status: "running", results: [], current: null, error: null });
    try {
      await api.start({ configs: picked, testMode, sortMethod, group: group.trim(), colors });
    } catch (e) {
      set({ error: (e as Error).message, status: "stopped" });
    }
  },

  /** 停止测试:重启后端进程(后端无 /stop 接口),保留前端已收到的 results
      以便后续"恢复测试"基于这些结果跳过已测节点。 */
  async stopTest() {
    set({ status: "stopped", current: null });
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("restart_backend");
    } catch {
      /* 重启失败也不影响前端把 status 设为 stopped */
    }
  },

  /** 恢复测试:从 selected 中过滤掉已经在 results 里的节点(按 remarks 匹配),
      只把剩下的提交给后端。前端不清空 results,polling 时会把新结果合并进来。 */
  async resumeTest(testMode, sortMethod, colors) {
    const { configs, selected, results, group } = get();
    const tested = new Set(results.map((r) => r.remarks));
    const picked = [...selected]
      .sort((a, b) => a - b)
      .map((i) => configs[i])
      .filter((c) => c && !tested.has(c.config.remarks));
    if (!picked.length) return;
    set({ status: "running", current: null, error: null });
    try {
      await api.start({ configs: picked, testMode, sortMethod, group: group.trim(), colors });
    } catch (e) {
      set({ error: (e as Error).message, status: "stopped" });
    }
  },

  /** 拉取 /getresults 并合并到本地 results:同一 remarks 后端最新数据覆盖旧的,
      stopTest 之前测过、本轮没再测的节点会被保留下来,实现"恢复测试"的累计展示。 */
  async refreshOnce() {
    try {
      const r = await api.results();
      const cur =
        r.current && (r.current as NodeResult).remarks
          ? (r.current as NodeResult)
          : null;
      const map = new Map<string, NodeResult>();
      for (const x of get().results) map.set(x.remarks, x);
      for (const x of r.results) map.set(x.remarks, x);
      set({ status: r.status, results: [...map.values()], current: cur });
    } catch {
      /* 忽略偶发抖动 */
    }
  },
}));

let polling = false;
export function startPolling(intervalMs = 1200) {
  if (polling) return;
  polling = true;
  const tick = async () => {
    await useTest.getState().refreshOnce();
    setTimeout(tick, intervalMs);
  };
  tick();
}
