import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { MapPin, Wifi, RefreshCw } from "lucide-react";

interface GeoIpInfo {
  country: string;
  region: string;
  city: string;
  isp: string;
}

/** 展示本机出口公网地理位置/运营商。
    走 Rust 侧 ip-api.com 中文接口(强制 IPv4 出口),不展示 IP 地址。 */
export default function NetworkInfo({ compact = false }: { compact?: boolean }) {
  const [info, setInfo] = useState<GeoIpInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [err, setErr] = useState<string | null>(null);

  async function load() {
    setLoading(true);
    setErr(null);
    try {
      const r = await invoke<GeoIpInfo>("get_my_ip_info");
      setInfo(r);
    } catch (e) {
      setErr((e as Error).message ?? "查询失败");
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    load();
  }, []);

  const place = info
    ? [info.country, info.region, info.city]
        .map((s) => (s ?? "").trim())
        .filter(Boolean)
        .join(" ")
    : "";

  return (
    <div
      className={
        "flex items-center gap-4 text-xs text-fg-muted " +
        (compact ? "" : "px-4 py-2 rounded-xl border border-border bg-bg-elev")
      }
    >
      <span className="inline-flex items-center gap-1.5">
        <MapPin size={12} />
        {loading
          ? "正在获取本机位置…"
          : err
            ? "位置获取失败"
            : place || "未知位置"}
      </span>
      {info?.isp && (
        <span className="inline-flex items-center gap-1.5">
          <Wifi size={12} />
          {info.isp}
        </span>
      )}
      <button
        onClick={load}
        disabled={loading}
        title="刷新"
        className="ml-auto inline-flex items-center justify-center w-6 h-6 rounded-md hover:bg-border/40 disabled:opacity-40"
      >
        <RefreshCw size={12} className={loading ? "animate-spin" : ""} />
      </button>
    </div>
  );
}
