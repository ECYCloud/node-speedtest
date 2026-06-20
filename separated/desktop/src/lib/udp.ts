// UDP 支持等级映射 - 与后端 src/renderer_v2.cpp::udpSupportLevel() 保持同步。
//
// 后端 nodeInfo.natType 来自 STUN(RFC 3489) 检测，取值是 ntt.cpp 的 NAT_TYPE_STR
// 英文枚举。前端结果表与历史记录 PNG 都用这套同源映射,UI 文案与图片文案一致。
//
// 等级语义(从高到低):
//   FullCone           完全支持   (Endpoint-Independent,P2P/UDP 应用最稳)
//   RestrictedCone     受限支持   (Address-Restricted)
//   PortRestrictedCone 端口受限   (Address-and-Port-Restricted)
//   Symmetric          对称受限   (端口随对端变化,P2P 几乎不可穿透)
//   Blocked            不支持     (UDP 直接被丢弃)
//   Unknown            未知       (test_nat_type=false 或检测失败)

export type UdpLevelTone =
  | "success"   // 完全支持
  | "primary"   // 受限支持(次优，与"主体可用"语义一致)
  | "warning"   // 端口受限
  | "danger"    // 对称受限 / 不支持
  | "neutral";  // 未知

export function udpLevelLabel(nat?: string): string {
  switch (nat) {
    case "FullCone":           return "完全支持";
    case "RestrictedCone":     return "受限支持";
    case "PortRestrictedCone": return "端口受限";
    case "Symmetric":          return "对称受限";
    case "Blocked":            return "不支持";
    default:                   return "未知";
  }
}

export function udpLevelTone(nat?: string): UdpLevelTone {
  switch (nat) {
    case "FullCone":           return "success";
    case "RestrictedCone":     return "primary";
    case "PortRestrictedCone": return "warning";
    case "Symmetric":          return "danger";
    case "Blocked":            return "danger";
    default:                   return "neutral";
  }
}
