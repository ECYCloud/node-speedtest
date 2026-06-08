#ifndef VERSION_H_INCLUDED
#define VERSION_H_INCLUDED

#define VERSION "v0.7.2"

// Fallback mihomo kernel version, used in the subscription User-Agent when the
// real version cannot be read from the bundled binary at runtime. Keep this in
// sync with tools/clients/mihomo.exe (query: `mihomo -v`).
#define MIHOMO_FALLBACK_VERSION "v1.19.27"

#endif // VERSION_H_INCLUDED
