import { Play, Loader2, Square } from "lucide-react";
import { useTest } from "../../store/test";
import { Button, Card, Select, SectionTitle } from "../../components/ui";

const TEST_MODES = [
  { value: "ALL", label: "延迟+下载" },
  { value: "TCP_PING", label: "仅延迟" },
];

// 后端 export_sort_method 支持: none / speed / rspeed / maxspeed / rmaxspeed / ping / rping。
// webgui_wrapper.cpp 把这里的 value 转小写后,把 "reverse_" 前缀替换成 "r",所以
// REVERSE_MAXSPEED → rmaxspeed、MAXSPEED → maxspeed。中间不能再夹下划线,否则替换不出后端识别的 key。
// 默认值是 REVERSE_MAXSPEED(最高速度倒序),与 base/pref.ini 的 export_sort_method=rmaxspeed 一致。
const SORT_METHODS = [
  { value: "REVERSE_MAXSPEED", label: "最高速度↓" },
  { value: "MAXSPEED", label: "最高速度↑" },
  { value: "REVERSE_SPEED", label: "平均速度↓" },
  { value: "SPEED", label: "平均速度↑" },
  { value: "REVERSE_PING", label: "延迟↓" },
  { value: "PING", label: "延迟↑" },
  { value: "NONE", label: "原始顺序" },
];

export default function ControlBar() {
  const {
    selected, results, status,
    testMode, sortMethod,
    setTestMode, setSortMethod,
    startTest, stopTest,
  } = useTest();

  // 三态对应三种按钮形态:
  //   running  → "停止"(可点)
  //   stopping → "停止中"(loader,disabled) — 前端已发停止指令,等待后端 batchTest
  //              真正退出循环并被 polling 拉到。这段时间不切回"开始测速",避免闪烁。
  //   stopped  → "开始测速"(可点,选中节点 > 0)
  const running = status === "running";
  const stopping = status === "stopping";

  return (
    <Card className="p-5">
      <SectionTitle desc="选择测试模式后开始测速，运行中可在右侧查看实时进度">
        测试控制
      </SectionTitle>
      <div className="grid grid-cols-1 md:grid-cols-2 gap-3 mb-4">
        <Field label="测试模式">
          <Select
            value={testMode}
            onChange={(v) => setTestMode(v as typeof testMode)}
            options={TEST_MODES}
            disabled={running || stopping}
          />
        </Field>
        <Field label="排序方式">
          <Select
            value={sortMethod}
            onChange={setSortMethod}
            options={SORT_METHODS}
            disabled={running || stopping}
          />
        </Field>
      </div>
      <div className="flex items-center justify-between gap-3 flex-wrap">
        <div className="text-xs text-fg-muted min-w-0 flex-1">
          {running
            ? `共 ${selected.size} 个节点中，已完成 ${results.length}`
            : stopping
              ? "正在停止当前任务…"
              : selected.size > 0
                ? `将测试 ${selected.size} 个节点`
                : "请先在节点列表中勾选节点"}
        </div>
        <div className="flex items-center gap-2 shrink-0">
          {(running || stopping) ? (
            <Button
              variant="danger"
              onClick={stopTest}
              disabled={stopping}
              className="min-w-[5.5rem] justify-center"
            >
              {stopping ? (
                <Loader2 size={14} className="animate-spin" />
              ) : (
                <Square size={14} />
              )}
              {stopping ? "停止中" : "停止"}
            </Button>
          ) : (
            <Button
              variant="primary"
              disabled={selected.size === 0}
              onClick={() => startTest(testMode, sortMethod)}
              className="min-w-[6.5rem] justify-center"
            >
              <Play size={14} />
              开始测速
            </Button>
          )}
        </div>
      </div>
    </Card>
  );
}

function Field({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <label className="flex flex-col gap-1.5">
      <span className="text-xs text-fg-muted">{label}</span>
      {children}
    </label>
  );
}
