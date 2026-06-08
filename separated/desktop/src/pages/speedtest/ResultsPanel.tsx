import { useTest } from "../../store/test";
import { Badge, Card, SectionTitle } from "../../components/ui";
import { fmtPingSeconds, fmtSpeed, fmtBytes } from "../../lib/format";
import { computeCurSpeed, computeMaxSpeed } from "../../lib/speed";
import { CheckCircle2, Activity, Inbox } from "lucide-react";

export default function ResultsPanel() {
  const { results, current, status } = useTest();
  const running = status === "running";

  // 实时速度与最高速度都基于 rawSocketSpeed 数组用同一套滑动窗口算法在前端计算,
  // 两者严格同源同尺度,UI 上"实时速度峰值"等于"本节点最高",不受 polling 抽样
  // 频率影响 — 后端把 0.5s 全量采样保留在数组里,前端拿到数组就拿到全部历史。
  // 详见 lib/speed.ts。
  const cur = current && current.remarks ? current : null;
  const curPing = cur?.gPing ?? 0;
  const curRaw = cur?.rawSocketSpeed;
  const curSpeed = computeCurSpeed(curRaw);
  const curMaxSpeed = computeMaxSpeed(curRaw);

  return (
    <Card className="p-5 flex flex-col">
      <SectionTitle
        desc={
          running
            ? "正在测试，实时更新当前节点延迟与下载速度"
            : results.length > 0
              ? `共 ${results.length} 个节点已完成`
              : undefined
        }
        right={
          running ? (
            <Badge variant="primary" className="gap-1">
              <Activity size={12} />
              运行中
            </Badge>
          ) : results.length > 0 ? (
            <Badge variant="success" className="gap-1">
              <CheckCircle2 size={12} />
              已完成
            </Badge>
          ) : null
        }
      >
        测试结果
      </SectionTitle>

      {running && cur && (
        <div className="mb-3 px-4 py-3 rounded-lg bg-primary/5 border border-primary/20 grid grid-cols-2 md:grid-cols-4 gap-3 text-sm">
          <div className="col-span-2 md:col-span-1 min-w-0">
            <div className="text-fg-muted text-xs mb-0.5">正在测试</div>
            <div className="font-medium truncate">{cur.remarks}</div>
          </div>
          <div>
            <div className="text-fg-muted text-xs mb-0.5">实时延迟</div>
            <div className="font-medium tabular-nums">
              {curPing > 0 ? fmtPingSeconds(curPing) : "--"}
            </div>
          </div>
          <div>
            <div className="text-fg-muted text-xs mb-0.5">实时速度</div>
            <div className="font-medium tabular-nums">
              {curSpeed > 0 ? fmtSpeed(curSpeed) : "--"}
            </div>
          </div>
          <div>
            <div className="text-fg-muted text-xs mb-0.5">本节点最高</div>
            <div className="font-medium tabular-nums">
              {curMaxSpeed > 0 ? fmtSpeed(curMaxSpeed) : "--"}
            </div>
          </div>
        </div>
      )}

      {!running && results.length === 0 ? (
        <div className="flex flex-col items-center justify-center text-fg-muted text-sm gap-2 py-10">
          <Inbox size={28} className="opacity-60" />
          <span>导入节点后开始测速，结果会出现在这里</span>
        </div>
      ) : (
        <div className="overflow-auto rounded-lg border border-border max-h-[480px]">
          <table className="w-full text-sm">
            <thead className="text-fg-muted sticky top-0 z-10">
              <tr className="text-left bg-bg-elev border-b border-border">
                <th className="py-2 px-3 bg-bg-elev">备注</th>
                <th className="py-2 px-3 bg-bg-elev">分组</th>
                <th className="py-2 px-3 bg-bg-elev">延迟</th>
                <th className="py-2 px-3 bg-bg-elev">丢包</th>
                <th className="py-2 px-3 bg-bg-elev">平均速度</th>
                <th className="py-2 px-3 bg-bg-elev">最高速度</th>
                <th className="py-2 px-3 bg-bg-elev">消耗流量</th>
              </tr>
            </thead>
            <tbody>
              {results.map((r, i) => (
                <tr
                  key={i}
                  className="border-t border-border hover:bg-border/20"
                >
                  <td className="py-2 px-3 truncate max-w-[260px]">
                    {r.remarks}
                  </td>
                  <td className="py-2 px-3 text-fg-muted">{r.group}</td>
                  <td className="py-2 px-3 tabular-nums">
                    {fmtPingSeconds(r.gPing)}
                  </td>
                  <td className="py-2 px-3 tabular-nums">
                    {(r.loss * 100).toFixed(0)}%
                  </td>
                  <td className="py-2 px-3 font-medium tabular-nums">
                    {fmtSpeed(r.dspeed)}
                  </td>
                  <td className="py-2 px-3 font-medium tabular-nums">
                    {/* 与"实时速度"同源:基于 rawSocketSpeed 用同一套滑动窗口算法,
                        而非读后端 dspeedMax 字段。已完成节点的 rawSpeed 数组是终态,
                        前后端基于同份数据算结果数学等价;但前端同源能保证测速过程中
                        实时进度区"本节点最高"与表格列字面值严格一致。 */}
                    {fmtSpeed(computeMaxSpeed(r.rawSocketSpeed))}
                  </td>
                  <td className="py-2 px-3 text-fg-muted tabular-nums">
                    {fmtBytes(r.trafficUsed)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </Card>
  );
}
