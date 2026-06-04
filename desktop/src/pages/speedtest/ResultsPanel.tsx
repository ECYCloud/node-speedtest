import { useTest } from "../../store/test";
import { Badge, Card, SectionTitle } from "../../components/ui";
import { fmtPingSeconds, fmtSpeed, fmtBytes } from "../../lib/format";
import { CheckCircle2, Activity, Inbox } from "lucide-react";

export default function ResultsPanel() {
  const { results, current, status } = useTest();
  const running = status === "running";

  return (
    <Card className="p-5 flex flex-col">
      <SectionTitle
        desc={
          running
            ? "正在测试,实时更新"
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

      {running && current?.remarks && (
        <div className="mb-3 px-3 py-2 rounded-lg bg-primary/5 border border-primary/20 text-sm">
          <span className="text-fg-muted">正在测试:</span>{" "}
          <span className="font-medium">{current.remarks}</span>
        </div>
      )}

      {!running && results.length === 0 ? (
        <div className="flex flex-col items-center justify-center text-fg-muted text-sm gap-2 py-10">
          <Inbox size={28} className="opacity-60" />
          <span>导入节点后开始测速,结果会出现在这里</span>
        </div>
      ) : (
        <div className="overflow-auto rounded-lg border border-border max-h-[480px]">
          <table className="w-full text-sm">
            <thead className="text-fg-muted sticky top-0 z-10">
              <tr className="text-left bg-bg-elev border-b border-border">
                <th className="py-2 px-3 bg-bg-elev">备注</th>
                <th className="py-2 px-3 bg-bg-elev">分组</th>
                <th className="py-2 px-3 bg-bg-elev">延迟</th>
                <th className="py-2 px-3 bg-bg-elev">HTTPS</th>
                <th className="py-2 px-3 bg-bg-elev">丢包</th>
                <th className="py-2 px-3 bg-bg-elev">下载</th>
                <th className="py-2 px-3 bg-bg-elev">流量</th>
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
                  <td className="py-2 px-3">{fmtPingSeconds(r.ping)}</td>
                  <td className="py-2 px-3">{fmtPingSeconds(r.gPing)}</td>
                  <td className="py-2 px-3">
                    {(r.loss * 100).toFixed(0)}%
                  </td>
                  <td className="py-2 px-3 font-medium">
                    {fmtSpeed(r.dspeed)}
                  </td>
                  <td className="py-2 px-3 text-fg-muted">
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
