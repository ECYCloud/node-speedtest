import { useTest } from "../../store/test";
import { Badge, Card, SectionTitle } from "../../components/ui";
import { fmtPingSeconds, fmtSpeed, fmtBytes } from "../../lib/format";
import { computeCurSpeed, computeMaxSpeed } from "../../lib/speed";
import { udpLevelLabel, udpLevelTone } from "../../lib/udp";
import { CheckCircle2, Activity, Inbox } from "lucide-react";

export default function ResultsPanel() {
  const { results, current, status, targetCount } = useTest();
  const running = status === "running";
  // 已完成的判定:跑完了所有选中节点。targetCount 为 0 时(初次进入页面、未开始测试)
  // 不该显示"已完成"——回退到旧的 results.length > 0 兜底。
  // results.length < targetCount → 中途异常或用户停止 → 显示"未完成 (n/m)"。
  const finished = !running && targetCount > 0 && results.length >= targetCount;
  const interrupted = !running && targetCount > 0 && results.length < targetCount;

  // 实时速度：短窗平滑；最高速度：rawSocketSpeed 峰值（与后端 dspeedMax 同源）。
  const cur = current && current.remarks ? current : null;
  // 实时延迟用后端累进均值(gPing)；次数以完整探测数组 rawGooglePingStatus 为准
  const curPing = cur ? cur.gPing || cur.ping || 0 : 0;
  const pingSamples = (
    cur?.rawGooglePingStatus?.length
      ? cur.rawGooglePingStatus
      : cur?.rawTcpPingStatus ?? []
  ).filter((x) => x > 0).length;
  const curRaw = cur?.rawSocketSpeed;
  const curSpeed = computeCurSpeed(curRaw);
  // 与结果表同源：优先 dspeedMax，否则 raw 峰值
  const curMaxSpeed =
    (cur?.dspeedMax ?? 0) > 0 ? (cur!.dspeedMax as number) : computeMaxSpeed(curRaw);



  return (
    <Card className="p-5 flex flex-col">
      <SectionTitle
        desc={
          running
            ? "正在测试，实时更新当前节点延迟与下载速度"
            : interrupted
              ? `共 ${targetCount} 个节点，仅完成 ${results.length}(测试中断)`
              : finished
                ? `共 ${results.length} 个节点已完成`
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
          ) : interrupted ? (
            <Badge variant="warning" className="gap-1">
              <Inbox size={12} />
              未完成
            </Badge>
          ) : finished || results.length > 0 ? (
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
            <div className="text-fg-muted text-xs mb-0.5">
              实时延迟{pingSamples > 0 ? ` · ${pingSamples}次` : ""}
            </div>
            <div className="font-medium tabular-nums">
              {curPing > 0 ? fmtPingSeconds(curPing) : "测量中…"}
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
          <span>
            {interrupted
              ? `已停止，未完成任何节点 (0/${targetCount})`
              : "导入节点后开始测速，结果会出现在这里"}
          </span>
        </div>
      ) : (
        <div className="overflow-auto rounded-lg border border-border max-h-[480px]">
          <table className="w-full text-sm">
            <thead className="text-fg-muted sticky top-0 z-10">
              <tr className="text-left bg-bg-elev border-b border-border">
                <th className="py-2 px-3 bg-bg-elev">备注</th>
                <th className="py-2 px-3 bg-bg-elev">分组</th>
                <th className="py-2 px-3 bg-bg-elev">平均延迟</th>
                <th className="py-2 px-3 bg-bg-elev">丢包</th>
                <th className="py-2 px-3 bg-bg-elev">平均速度</th>
                <th className="py-2 px-3 bg-bg-elev">最高速度</th>
                <th className="py-2 px-3 bg-bg-elev">UDP</th>
                <th className="py-2 px-3 bg-bg-elev">消耗流量</th>
              </tr>
            </thead>
            <tbody>
              {results.map((r, i) => (
                <tr
                  key={`${r.group}|${r.remarks}|${r.geoIP?.inbound?.address ?? i}`}
                  className="border-t border-border hover:bg-border/20"
                >
                  <td className="py-2 px-3 truncate max-w-[260px]">
                    {r.remarks}
                  </td>
                  <td className="py-2 px-3 text-fg-muted">{r.group}</td>
                  <td className="py-2 px-3 tabular-nums">
                    {fmtPingSeconds(r.gPing || r.ping || 0)}
                  </td>
                  <td className="py-2 px-3 tabular-nums">
                    {(r.loss * 100).toFixed(0)}%
                  </td>
                  <td className="py-2 px-3 font-medium tabular-nums">
                    {fmtSpeed(r.dspeed)}
                  </td>
                  <td className="py-2 px-3 font-medium tabular-nums">
                    {/* 与测速中「本节点最高」同源：rawSocketSpeed 峰值；
                        dspeedMax 应由后端从同一数组得出，缺省时前端自算。 */}
                    {fmtSpeed(
                      r.dspeedMax > 0 ? r.dspeedMax : computeMaxSpeed(r.rawSocketSpeed)
                    )}
                  </td>
                  <td className="py-2 px-3">
                    <Badge variant={udpLevelTone(r.natType)}>
                      {udpLevelLabel(r.natType)}
                    </Badge>
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
