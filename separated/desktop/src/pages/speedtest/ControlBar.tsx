import { useState } from "react";
import { Play, Loader2, Square } from "lucide-react";
import { useTest } from "../../store/test";
import { Button, Card, Select, SectionTitle } from "../../components/ui";

const TEST_MODES = [
  { value: "ALL", label: "延迟+下载" },
  { value: "TCP_PING", label: "仅延迟" },
];

const SORT_METHODS = [
  { value: "REVERSE_SPEED", label: "速度↓" },
  { value: "SPEED", label: "速度↑" },
  { value: "REVERSE_PING", label: "延迟↓" },
  { value: "PING", label: "延迟↑" },
  { value: "ORIGINAL", label: "原始顺序" },
];

export default function ControlBar() {
  const [stopping, setStopping] = useState(false);
  const {
    selected, results, status,
    testMode, sortMethod,
    setTestMode, setSortMethod,
    startTest, stopTest,
  } = useTest();

  const running = status === "running";

  async function onStop() {
    setStopping(true);
    try {
      await stopTest();
    } finally {
      setStopping(false);
    }
  }

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
            disabled={running}
          />
        </Field>
        <Field label="排序方式">
          <Select
            value={sortMethod}
            onChange={setSortMethod}
            options={SORT_METHODS}
            disabled={running}
          />
        </Field>
      </div>
      <div className="flex items-center justify-between gap-3 flex-wrap">
        <div className="text-xs text-fg-muted min-w-0 flex-1">
          {running
            ? `共 ${selected.size} 个节点中，已完成 ${results.length}`
            : selected.size > 0
              ? `将测试 ${selected.size} 个节点`
              : "请先在节点列表中勾选节点"}
        </div>
        <div className="flex items-center gap-2 shrink-0">
          {running ? (
            <Button
              variant="danger"
              onClick={onStop}
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
