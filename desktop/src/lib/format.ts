/** 字节/字节每秒的友好格式化 */
export function fmtSpeed(bps: number): string {
  if (!bps || bps <= 0) return "—";
  const units = ["B/s", "KB/s", "MB/s", "GB/s"];
  let v = bps;
  let u = 0;
  while (v >= 1024 && u < units.length - 1) {
    v /= 1024;
    u++;
  }
  return `${v.toFixed(v >= 100 ? 0 : v >= 10 ? 1 : 2)} ${units[u]}`;
}

/** 秒为单位的延迟,后端接口里 ping 值字段已是秒 */
export function fmtPingSeconds(s: number): string {
  if (!s || s <= 0) return "—";
  return `${(s * 1000).toFixed(0)} ms`;
}

export function fmtBytes(n: number): string {
  if (!n || n <= 0) return "—";
  const units = ["B", "KB", "MB", "GB"];
  let v = n;
  let u = 0;
  while (v >= 1024 && u < units.length - 1) {
    v /= 1024;
    u++;
  }
  return `${v.toFixed(v >= 100 ? 0 : 1)} ${units[u]}`;
}

export function fmtTime(ms: number): string {
  if (!ms) return "—";
  const d = new Date(ms);
  const pad = (x: number) => String(x).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}
