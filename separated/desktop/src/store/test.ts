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

/** 测试状态机:
    - "stopped"  完全空闲(初始/批次结束/手动停止已落地)
    - "running"  后端正在跑节点
    - "stopping" 用户已点停止,但后端 batchTest detached thread 还没真退出
                 (下载累积循环要 ~500ms 才检查 stop_requested,polling 也要 ~1.2s
                 才能拉到后端 start_flag=false)。这段过渡期前端按钮锁在"停止中",
                 polling 拿到的 status=running 不允许覆盖回去 — 否则会出现
                 "开始测速→停止→开始测速"的闪烁。 */
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
  /** 本轮测试预期的节点总数 = startTest 时选中的节点数。
      用于 ResultsPanel 区分"已完成(全部测完)"和"未完成(中途停了/异常退出)"。
      stopped 且 results.length === targetCount → 已完成;否则 → 未完成 (n/m)。 */
  targetCount: number;
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
    set({ status: "running", results: [], current: null, error: null, targetCount: picked.length });
    try {
      await api.start({ configs: picked, testMode, sortMethod, group: group.trim() });
    } catch (e) {
      set({ error: (e as Error).message, status: "stopped" });
    }
  },

  /** 停止测试:发 POST /stop,后端在 batchTest 节点循环间检测到 stop_requested 后跳出,
      保留 allNodes/targetNodes,用户可立即点"开始"重测同一份订阅。
      旧实现走 Tauri 命令 restart_backend 杀后端进程,会清空内存里的节点列表 →
      再点开始 0 节点,表现为"任何节点都无法测试"。

      状态过渡:running → stopping(立即) → stopped(由 refreshOnce 拉到后端 stopped 时切)。
      不能直接 set stopped,否则 polling 在中间窗口拉到后端仍 running 会把 status 倒回去,
      表现为按钮"开始测速→停止→开始测速"闪烁。 */
  async stopTest() {
    if (get().status !== "running") return;
    set({ status: "stopping" });
    try {
      await api.stop();
    } catch {
      /* 即便请求失败 polling 仍会拉到真实状态最终落到 stopped */
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
      // 状态合并规则:
      //   stopping 中间态:只允许过渡到 stopped(后端真停了),后端仍 running 时
      //     保持 stopping —— 防止"用户已点停止 → 按钮短暂跳回'停止' → 又跳回'开始'"闪烁。
      //   其他状态:直接采用后端真实状态。
      const cur_status = get().status;
      const next_status: TestStore["status"] =
        cur_status === "stopping"
          ? r.status === "stopped" ? "stopped" : "stopping"
          : r.status;
      // stopping 过渡期不刷新 current —— 保持停止瞬间的"已完成"语义,避免
      // 又出现一个正在测试的节点卡片闪一下。
      const next_current = cur_status === "stopping" ? null : cur;
      set({ status: next_status, results: [...map.values()], current: next_current });
    } catch {
      /* 忽略偶发抖动 */
    }
  },
}));

let polling = false;
export function startPolling(intervalMs = 500) {
  // 500ms 与后端 perform_test 累积循环 sleep(500) 完全对齐:后端每 0.5s 写一格
  // rawSpeed 并刷新 maxSpeed/avgSpeed,前端按同频率拉就不会漏掉任何窗口快照,
  // 用户观察到的"实时速度峰值"严格等于后端记录的"最高速度"。
  // 旧值 1200ms 会让前端只看到后端 ~一半的窗口,峰值若落在 polling 间隙就会
  // 永远看不到 → 体感"最高速度比观察到的实时峰值高几个百分点"。
  // 后端 /getresults 是本地 HTTP,~kB 级响应,500ms 频率没压力。
  if (polling) return;
  polling = true;
  const tick = async () => {
    await useTest.getState().refreshOnce();
    setTimeout(tick, intervalMs);
  };
  tick();
}
