import { useTest } from "../../store/test";
import { Badge, Card, SectionTitle } from "../../components/ui";
import { fmtPingSeconds, fmtSpeed, fmtBytes } from "../../lib/format";
import { CheckCircle2, Activity, Inbox } from "lucide-react";

export default function ResultsPanel() {
  const { results, current, status } = useTest();
  const running = status === "running";

  // 当前正在测试的节点的实时进度:gPing 是 HTTPS 延迟(秒)。
  // 实时速度策略:rawSocketSpeed 是后端每 0.5s 一格的瞬时采样(字节/秒),代理网络
  // 在 TLS 握手/慢启动/丢包重传时单格波动可达 10 倍。直接取最后一格瞬时会显得
  //   实时速度 ≪ 最高速度
  // ——其实是 0.5s 颗粒度 + 网络抖动的真实表现,但反直觉。
  // 改取最近 4 格(2s)非零采样的均值,平掉单格抖动,数值更贴近代理真实带宽。
  // 全为零(刚启动/已停)时返回 0,保持"--"占位。
  const cur = current && current.remarks ? current : null;
  const curPing = cur?.gPing ?? 0;
  const rawSpeeds = cur?.rawSocketSpeed ?? [];
  let curSpeed = 0;
  {
    const SLIDING_WINDOW = 4; // 4 × 0.5s = 2s
    let sum = 0;
    let count = 0;
    for (let i = rawSpeeds.length - 1; i >= 0 && count < SLIDING_WINDOW; i--) {
      if (rawSpeeds[i] > 0) {
        sum += rawSpeeds[i];
        count++;
      }
    }
    if (count > 0) curSpeed = sum / count;
  }

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
        <div className="mb-3 px-4 py-3 rounded-lg bg-primary/5 border border-primary/20 grid grid-cols-1 md:grid-cols-3 gap-3 text-sm">
          <div className="md:col-span-1 min-w-0">
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
                    {fmtSpeed(r.dspeedMax ?? 0)}
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
