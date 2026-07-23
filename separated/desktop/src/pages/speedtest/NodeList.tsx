import { useMemo } from "react";
import { Search } from "lucide-react";
import { useTest } from "../../store/test";
import { Badge, Card, Input, SectionTitle, Button } from "../../components/ui";
import { cn } from "../../lib/cn";

export default function NodeList() {
  const {
    configs,
    selected,
    filter,
    typeFilter,
    group,
    status,
    setFilter,
    toggleSelect,
    selectAll,
    clearSelect,
    toggleType,
  } = useTest();
  const locked = status === "running" || status === "stopping";

  const types = useMemo(() => {
    const s = new Set<string>();
    configs.forEach((c) => s.add(c.type));
    return [...s];
  }, [configs]);

  const filtered = useMemo(() => {
    const f = filter.trim().toLowerCase();
    return configs
      .map((c, i) => ({ c, i }))
      .filter(({ c }) => {
        if (typeFilter.size > 0 && !typeFilter.has(c.type)) return false;
        if (!f) return true;
        const cfg = c.config;
        return (
          (cfg?.remarks ?? "").toLowerCase().includes(f) ||
          (cfg?.group ?? "").toLowerCase().includes(f) ||
          (cfg?.server ?? "").toLowerCase().includes(f)
        );
      });
  }, [configs, filter, typeFilter]);

  if (!configs.length) return null;

  const visibleIndices = filtered.map((x) => x.i);
  const allVisibleSelected =
    visibleIndices.length > 0 &&
    visibleIndices.every((i) => selected.has(i));

  return (
    <Card className="p-5 flex flex-col">
      <SectionTitle
        desc={`共 ${configs.length} 个节点 · 已选 ${selected.size}${
          filtered.length !== configs.length ? ` · 当前可见 ${filtered.length}` : ""
        }`}
        right={
          <div className="flex items-center gap-2">
            <Button
              size="sm"
              onClick={() => selectAll(visibleIndices)}
              disabled={locked || visibleIndices.length === 0 || allVisibleSelected}
              title="选中当前筛选可见的全部节点"
            >
              全选可见
            </Button>
            <Button size="sm" variant="ghost" onClick={clearSelect} disabled={locked}>
              清空
            </Button>
          </div>
        }
      >
        节点列表
      </SectionTitle>

      <div className="flex flex-wrap gap-2 mb-3">
        <div className="relative flex-1 min-w-[200px]">
          <Search
            size={14}
            className="absolute left-3 top-1/2 -translate-y-1/2 text-fg-muted"
          />
          <Input
            placeholder="按备注 / 分组 / 服务器筛选"
            className="pl-9"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
          />
        </div>
        <div className="flex items-center gap-1.5">
          {types.map((t) => (
            <button
              key={t}
              onClick={() => toggleType(t)}
              className={cn(
                "px-3 py-1 text-xs rounded-full border transition",
                typeFilter.has(t)
                  ? "bg-primary/10 border-primary text-primary"
                  : "border-border text-fg-muted hover:text-fg"
              )}
            >
              {t}
            </button>
          ))}
        </div>
      </div>

      <div className="overflow-auto rounded-lg border border-border max-h-[400px]">
        <table className="w-full text-sm">
          <thead className="text-fg-muted sticky top-0 z-10">
            <tr className="text-left bg-bg-elev border-b border-border">
              <th className="w-10 py-2 px-3 bg-bg-elev"></th>
              <th className="py-2 px-3 bg-bg-elev">备注</th>
              <th className="py-2 px-3 bg-bg-elev">分组</th>
              <th className="py-2 px-3 bg-bg-elev">类型</th>
              <th className="py-2 px-3 bg-bg-elev">服务器</th>
            </tr>
          </thead>
          <tbody>
            {filtered.map(({ c, i }) => {
              const checked = selected.has(i);
              return (
                <tr
                  key={i}
                  onClick={() => !locked && toggleSelect(i)}
                  className={cn(
                    "border-t border-border",
                    locked ? "cursor-not-allowed opacity-80" : "cursor-pointer",
                    checked ? "bg-primary/5" : !locked && "hover:bg-border/20"
                  )}
                >
                  <td className="py-2 px-3">
                    <input
                      type="checkbox"
                      readOnly
                      checked={checked}
                      className="accent-primary"
                    />
                  </td>
                  <td className="py-2 px-3 truncate max-w-[260px]">
                    {c.config.remarks}
                  </td>
                  <td className="py-2 px-3 text-fg-muted">{group.trim() || c.config.group}</td>
                  <td className="py-2 px-3">
                    <Badge variant="primary">{c.type}</Badge>
                  </td>
                  <td className="py-2 px-3 text-fg-muted">
                    {c.config.server}:{c.config.server_port}
                  </td>
                </tr>
              );
            })}
            {filtered.length === 0 && (
              <tr>
                <td
                  colSpan={5}
                  className="py-8 text-center text-fg-muted text-sm"
                >
                  没有匹配的节点
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </Card>
  );
}
