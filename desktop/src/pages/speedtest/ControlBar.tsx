import { useState } from "react";
import { Play, Loader2, Square, RotateCcw } from "lucide-react";
import { useTest } from "../../store/test";
import { Button, Card, Select, SectionTitle } from "../../components/ui";

const TEST_MODES = [
  { value: "ALL", label: "完整测速(延迟 + 下载)" },
  { value: "TCP_PING", label: "仅延迟" },
];

const SORT_METHODS = [
  { value: "REVERSE_SPEED", label: "速度由高到低" },
  { value: "SPEED", label: "速度由低到高" },
  { value: "REVERSE_PING", label: "延迟由高到低" },
  { value: "PING", label: "延迟由低到高" },
  { value: "ORIGINAL", label: "原始顺序" },
];

const COLOR_STYLES = [
  { value: "rainbow", label: "彩虹" },
  { value: "original", label: "原版" },
];

export default function ControlBar() {
  const [testMode, setTestMode] = useState<"ALL" | "TCP_PING">("ALL");
  const [sortMethod, setSortMethod] = useState("REVERSE_SPEED");
  const [colors, setColors] = useState("rainbow");
  const [stopping, setStopping] = useState(false);
  const { configs, selected, results, status, startTest, resumeTest, stopTest } = useTest();

  const running = status === "running";

  // 计算"剩余可恢复"的节点数:在 selected 中、且还没有结果的节点
  const testedRemarks = new Set(results.map((r) => r.remarks));
  const remaining = [...selected]
    .map((i) => configs[i])
    .filter((c) => c && !testedRemarks.has(c.config.remarks)).length;
  const canResume = !running && results.length > 0 && remaining > 0;

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
      <SectionTitle desc="选择测试模式后开始测速,运行中可在右侧查看实时进度">
        测试控制
      </SectionTitle>
      <div className="grid grid-cols-1 md:grid-cols-3 gap-3 mb-4">
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
        <Field label="结果配色">
          <Select
            value={colors}
            onChange={setColors}
            options={COLOR_STYLES}
            disabled={running}
          />
        </Field>
      </div>
      <div className="flex items-center justify-between gap-3 flex-wrap">
        <div className="text-xs text-fg-muted min-w-0 flex-1">
          {running
            ? `共 ${selected.size} 个节点中,已完成 ${results.length}`
            : canResume
              ? `还有 ${remaining} 个节点未测,可恢复继续`
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
            <>
              {canResume && (
                <Button
                  variant="secondary"
                  onClick={() =>
                    resumeTest(testMode, sortMethod, "", colors)
                  }
                  className="min-w-[6rem] justify-center"
                  title="跳过已测节点,继续测试剩下的"
                >
                  <RotateCcw size={14} />
                  恢复测试
                </Button>
              )}
              <Button
                variant="primary"
                disabled={selected.size === 0}
                onClick={() => startTest(testMode, sortMethod, "", colors)}
                className="min-w-[6.5rem] justify-center"
              >
                <Play size={14} />
                {results.length > 0 ? "重新测速" : "开始测速"}
              </Button>
            </>
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
