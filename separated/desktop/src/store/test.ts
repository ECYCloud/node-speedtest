import { create } from "zustand";
import { api, NodeConfig, NodeResult, StartParams } from "../lib/api";

/** 后端在解析失败时可能返回 type=Unknown 且没有 config 字段的占位项，
    那种节点缺少 server/remarks 等关键信息，无法用于测速，直接丢弃。
    其他协议(SS/SSR/V2Ray/Trojan/Snell/VLESS/Hysteria2/AnyTLS/...)只要带完整 config 都保留。 */
function sanitizeConfigs(list: unknown): NodeConfig[] {
  if (!Array.isArray(list)) return [];
  return list.filter((x): x is NodeConfig => {
    if (!x || typeof x !== "object") return false;
    const node = x as Partial<NodeConfig>;
    if (typeof node.type !== "string") return false;
    // 没有 config 字段的(后端 Unknown 分支)直接丢，无法测速
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
      在导入节点时填写，留空则后端用第一个节点的协议默认 group。 */
  group: string;
  /** 以下为界面持久状态:放在 store 而非组件本地 useState，这样切换页面
      (Speedtest 组件卸载)再切回来时不会丢失/重置。 */
  url: string;                         // 订阅链接输入框
  importTab: "sub" | "file";           // 导入方式 Tab
  testMode: "ALL" | "TCP_PING";        // 测试模式
  sortMethod: string;                  // 排序方式

  loadSubscription: (url: string) => Promise<void>;
  loadFileConfig: (fileName: string, fileBytes: number[]) => Promise<void>;
  /** 由本地文件路径导入:Rust 端读字节 + 上传后端，错误会写入 error 显示给用户 */
  loadFileByPath: (path: string) => Promise<void>;
  toggleSelect: (i: number) => void;
  /** 全选:不传参表示选中全部 configs;传可见 indices 时只选这些 */
  selectAll: (indices?: number[]) => void;
  clearSelect: () => void;
  setFilter: (s: string) => void;
  toggleType: (t: string) => void;
  setGroup: (g: string) => void;
  setUrl: (s: string) => void;
  setImportTab: (t: "sub" | "file") => void;
  setTestMode: (m: "ALL" | "TCP_PING") => void;
  setSortMethod: (s: string) => void;
  startTest: (
    testMode: StartParams["testMode"],
    sortMethod: string
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
  url: "",
  importTab: "sub",
  testMode: "ALL",
  sortMethod: "REVERSE_MAXSPEED",

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

  async loadFileByPath(path) {
    set({ loadingConfigs: true, error: null });
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      // Rust 端读文件 + multipart 上传后端，返回节点 JSON 文本(或 "error"/"running")
      const text = await invoke<string>("import_config_file", { path });
      const trimmed = (text ?? "").trim();
      if (trimmed === "error") throw new Error("后端无法识别该配置文件");
      if (trimmed === "running") throw new Error("后端正在测速中，请等当前任务结束");
      let list: unknown;
      try {
        list = JSON.parse(trimmed);
      } catch {
        throw new Error(`配置文件解析失败: ${trimmed.slice(0, 80)}`);
      }
      const safe = sanitizeConfigs(list);
      if (safe.length === 0) throw new Error("未从该文件解析到可测速的节点");
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
    // 仅记录自定义分组名;不回写 config.group(那是后端匹配节点的键，
    // 改了会导致 ssrspeed_regenerate_node_list 匹配不到节点 → 0 节点 → 无法测试)。
    // NodeList 用 store.group 做只读展示，真正的分组覆盖由 /start 的 group 参数在后端完成。
    set({ group: g });
  },
  setUrl(s) { set({ url: s }); },
  setImportTab(t) { set({ importTab: t }); },
  setTestMode(m) { set({ testMode: m }); },
  setSortMethod(s) { set({ sortMethod: s }); },

  async startTest(testMode, sortMethod) {
    const { configs, selected, group } = get();
    const picked = [...selected]
      .sort((a, b) => a - b)
      .map((i) => configs[i])
      .filter(Boolean);
    if (!picked.length) return;
    set({ status: "running", results: [], current: null, error: null });
    try {
      await api.start({ configs: picked, testMode, sortMethod, group: group.trim() });
    } catch (e) {
      set({ error: (e as Error).message, status: "stopped" });
    }
  },

  /** 停止测试:重启后端进程(后端无 /stop 接口)。 */
  async stopTest() {
    set({ status: "stopped", current: null });
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("restart_backend");
    } catch {
      /* 重启失败也不影响前端把 status 设为 stopped */
    }
  },

  /** 拉取 /getresults 并合并到本地 results:同一 remarks 后端最新数据覆盖旧的。
      注意:不把正在测试的 current 并入结果表 —— current 数据不全(延迟未测完时
      pkLoss 仍是默认 100%)，并入会让结果表出现"没测就显示100%丢包"的假数据。
      current 只用于"正在测试"卡片;测试结束后后端 results 已含全部节点(含最后一个)。 */
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
