#ifndef VERSION_H_INCLUDED
#define VERSION_H_INCLUDED

#define VERSION "v0.7.2"
// 与 VERSION 同步，但去掉前导 'v'。订阅请求 User-Agent 的软件标识段
// 约定 stairspeedtest-reborn/<X.Y.Z> 不带 v,与 mozilla/firefox 等 UA 习惯一致。
#define VERSION_NO_V "0.7.2"

// Fallback mihomo kernel version, used in the subscription User-Agent when the
// real version cannot be read from the bundled binary at runtime. Keep this in
// sync with tools/clients/mihomo.exe (query: `mihomo -v`).
#define MIHOMO_FALLBACK_VERSION "v1.19.29"

#endif // VERSION_H_INCLUDED
