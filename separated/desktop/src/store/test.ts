import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
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

/** 与后端 match_targets 四元组对齐，避免同备注节点互相覆盖。 */
function resultKey(r: NodeResult): string {
  const addr = r.geoIP?.inbound?.address ?? "";
  return `${r.group}\0${r.remarks}\0${addr}`;
}

/** 测试状态机:
    - "stopped"  完全空闲
    - "running"  引擎正在测节点
    - "stopping" 已请求停止，等待当前测量协作退出 */
interface TestStore {
  status: "stopped" | "running" | "stopping";
  configs: NodeConfig[];
  selected: Set<number>;
  results: NodeResult[];
  current: Partial<NodeResult> | null;
  filter: string;
  typeFilter: Set<string>;
  loadingConfigs: boolean;
  error: string | null;
  /** 正在发起 /start，防止连点并发 */
  starting: boolean;
  /** 本轮测试预期的节点总数 = startTest 时选中的节点数。
      用于 ResultsPanel 区分"已完成(全部测完)"和"未完成(中途停了/异常退出)"。
      stopped 且 results.length === targetCount → 已完成;否则 → 未完成 (n/m)。 */
  targetCount: number;
  /** 分组名:用于 PNG 标题、历史记录文件名前缀、统一所有节点的 group 字段。
      留空时后端统一为 Node Speedtest。 */
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
  starting: false,
  targetCount: 0,
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
      if (safe.length === 0) {
        set({
          error: "未识别到可测速节点。请粘贴订阅 URL，或 trojan/vless/ss 等单节点链接。",
          configs: [],
          selected: new Set(),
        });
        return;
      }
      set({
        configs: safe,
        selected: new Set(safe.map((_, i) => i)),
        results: [],
        current: null,
        error: null,
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
      if (safe.length === 0) {
        set({
          error: "未从该文件解析到可测速的节点",
          configs: [],
          selected: new Set(),
        });
        return;
      }
      set({
        configs: safe,
        selected: new Set(safe.map((_, i) => i)),
        results: [],
        current: null,
        error: null,
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
    const { status } = get();
    if (status === "running" || status === "stopping") return;
    const s = new Set(get().selected);
    s.has(i) ? s.delete(i) : s.add(i);
    set({ selected: s });
  },
  selectAll(indices) {
    const { status } = get();
    if (status === "running" || status === "stopping") return;
    if (indices !== undefined) {
      // 筛选为空时不可把已选清空（旧条件 length >= 0 恒真）
      if (indices.length === 0) return;
      set({ selected: new Set(indices) });
    } else {
      set({ selected: new Set(get().configs.map((_, i) => i)) });
    }
  },
  clearSelect() {
    const { status } = get();
    if (status === "running" || status === "stopping") return;
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
    // 不回写 config.group：匹配键仍是导入时的 group/remarks/server/port。
    set({ group: g });
  },
  setUrl(s) { set({ url: s }); },
  setImportTab(t) { set({ importTab: t }); },
  setTestMode(m) { set({ testMode: m }); },
  setSortMethod(s) { set({ sortMethod: s }); },

  async startTest(testMode, sortMethod) {
    const { configs, selected, group, status, starting } = get();
    if (starting || status === "running" || status === "stopping") return;
    const picked = [...selected]
      .sort((a, b) => a - b)
      .map((i) => configs[i])
      .filter(Boolean);
    if (!picked.length) return;
    set({ starting: true, error: null });
    try {
      const resp = (await api.start({
        configs: picked,
        testMode,
        sortMethod,
        group: group.trim(),
      })).trim();
      if (resp === "done") {
        set({ error: "上次测速刚刚结束，请稍候再试" });
        return;
      }
      if (resp === "busy") {
        set({ error: "测速正在进行中" });
        await get().refreshOnce();
        return;
      }
      if (resp === "running") {
        set({
          status: "running",
          results: [],
          current: null,
          error: null,
          targetCount: picked.length,
        });
        return;
      }
      if (resp) {
        set({ error: resp });
      }
    } catch (e) {
      set({ error: (e as Error).message });
    } finally {
      set({ starting: false });
    }
  },

  /** 停止测试：引擎协作取消；保留已导入节点以便立即重测。 */
  async stopTest() {
    if (get().status !== "running") return;
    set({ status: "stopping" });
    try {
      await api.stop();
    } catch {
      /* 即便请求失败 polling 仍会拉到真实状态最终落到 stopped */
    }
  },

  /** 拉取 /getresults 并合并到本地 results（键与后端 match_targets 四元组对齐）。
      注意:不把正在测试的 current 并入结果表 —— current 数据不全(延迟未测完时
      pkLoss 仍是默认 100%)，并入会让结果表出现"没测就显示100%丢包"的假数据。
      current 只用于"正在测试"卡片;测试结束后后端 results 已含全部节点(含最后一个)。 */
  async refreshOnce() {
    try {
      const r = await api.results();
      pollFailStreak = 0;
      const cur =
        r.current && (r.current as NodeResult).remarks
          ? (r.current as NodeResult)
          : null;
      const map = new Map<string, NodeResult>();
      for (const x of get().results) map.set(resultKey(x), x);
      for (const x of r.results) map.set(resultKey(x), x);
      // 状态合并规则:
      //   stopping 中间态:只允许过渡到 stopped(后端真停了),后端仍 running 时
      //     保持 stopping —— 防止"用户已点停止 → 按钮短暂跳回'停止' → 又跳回'开始'"闪烁。
      //   其他状态:直接采用后端真实状态。
      const cur_status = get().status;
      const next_status: TestStore["status"] =
        cur_status === "stopping"
          ? r.status === "stopped" ? "stopped" : "stopping"
          : r.status;
      // stopping 过渡期不刷新 current —— 保持停止瞬间的"已完成"语义，避免
      // 又出现一个正在测试的节点卡片闪一下。
      const next_current = cur_status === "stopping" ? null : cur;
      const backendTotal =
        typeof r.targetCount === "number" && r.targetCount > 0
          ? r.targetCount
          : undefined;
      const nextTarget =
        backendTotal ??
        (next_status === "running" || next_status === "stopping"
          ? Math.max(get().targetCount, r.results.length)
          : get().targetCount);
      set({
        status: next_status,
        results: [...map.values()],
        current: next_current,
        targetCount: nextTarget,
      });
    } catch {
      pollFailStreak += 1;
      // 连续失败时退出假 running，避免「停止」按钮与「未连接」并存卡死
      if (pollFailStreak >= 8) {
        const s = get().status;
        if (s === "running" || s === "stopping") {
          set({
            status: "stopped",
            current: null,
            error: "无法获取测速结果，引擎可能已断开",
          });
        }
      }
    }
  },
}));

let polling = false;
let pollFailStreak = 0;
export function startPolling(intervalMs = 400) {
  if (polling) return;
  polling = true;
  const tick = async () => {
    await useTest.getState().refreshOnce();
    // 测速中加快轮询，否则延迟累进采样容易被 400ms 间隔“跳过”
    const running = useTest.getState().status === "running";
    setTimeout(tick, running ? 150 : intervalMs);
  };
  tick();
}
