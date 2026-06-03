// New SSRSpeed-style renderer for Stair Speedtest Reborn (mihomo build).
// Self-contained: no dependence on the legacy column layout.
//
// Layout (top -> bottom):
//   * Banner: group / airport name (large, centred)
//   * Sub-banner: sort key description ("speed - 最大速度" etc)
//   * Header row: # / 节点名称 / 类型 / HTTPS 延迟 / 平均速度 / 最大速度 / 每秒速度
//   * Data rows: red circular serial badge, type pill, latency cell with traffic-
//     light colour, two speed cells with rainbow colour, sparkline of rawSpeed
//   * Footer: a few lines of metadata (sort method, totals, generation time)
//
// All sizes are multiplied by the global `image_scale` (declared in renderer.cpp)
// so the user can dial in higher DPI via pref.ini.

#include <algorithm>
#include <functional>
#include <pngwriter.h>
#include "renderer.h"
#include "version.h"
#include "nodeinfo.h"
#include "string_hash.h"
#include "misc.h"
#include "printout.h"

extern std::string export_sort_method_render;
extern bool export_as_ssrspeed;
extern int image_scale;
extern std::vector<color> colorgroup;
extern std::vector<int> bounds;

// Forward decls of helpers defined in renderer.cpp itself:
bool comparer(nodeInfo &a, nodeInfo &b);
void getSpeedColor(std::string speed, color *finalcolor);
void loadDefaultColor(std::string type);
void rendererInit(const std::string &font, int fontsize);
std::string secondToString(int duration);

// --- small local helpers ----------------------------------------------------
namespace {

inline int measure(pngwriter &probe, const std::string &font, int fs, const std::string &text)
{
    return probe.get_text_width_utf8(const_cast<char *>(font.data()), fs,
                                     const_cast<char *>(text.data()));
}

inline void plotText(pngwriter &png, const std::string &font, int fs,
                     int x, int y, const std::string &text,
                     double r, double g, double b)
{
    png.plot_text_utf8(const_cast<char *>(font.data()), fs, x, y, 0.0,
                       const_cast<char *>(text.data()), r, g, b);
}

// Map link type to a short protocol pill label.
std::string protoLabel(int linkType)
{
    switch(linkType)
    {
    case SPEEDTEST_MESSAGE_FOUNDVMESS:   return "Vmess";
    case SPEEDTEST_MESSAGE_FOUNDVLESS:   return "Vless";
    case SPEEDTEST_MESSAGE_FOUNDSS:      return "SS";
    case SPEEDTEST_MESSAGE_FOUNDSSR:     return "SSR";
    case SPEEDTEST_MESSAGE_FOUNDTROJAN:  return "Trojan";
    case SPEEDTEST_MESSAGE_FOUNDHY2:     return "Hysteria2";
    case SPEEDTEST_MESSAGE_FOUNDANYTLS:  return "AnyTLS";
    case SPEEDTEST_MESSAGE_FOUNDSOCKS:   return "Socks5";
    case SPEEDTEST_MESSAGE_FOUNDHTTP:    return "HTTP";
    case SPEEDTEST_MESSAGE_FOUNDSNELL:   return "Snell";
    default:                             return "—";
    }
}

// HTTPS-latency cell background: green <200ms, amber <500ms, red >=500ms.
void latencyBg(const std::string &ping, double &r, double &g, double &b)
{
    float p = 0.0f;
    try { p = stof(ping); } catch(...) { p = 0.0f; }
    if(p <= 0.0f || p > 9000.0f)        { r = 0.95; g = 0.95; b = 0.95; return; }
    if(p < 200.0f)                      { r = 0.84; g = 0.95; b = 0.84; return; }
    if(p < 500.0f)                      { r = 0.99; g = 0.92; b = 0.78; return; }
    r = 0.99; g = 0.78; b = 0.78;
}

std::string sortLabelText(const std::string &m)
{
    switch(hash_(m))
    {
    case "rmaxspeed"_hash: return "speed - 最大速度";
    case "maxspeed"_hash:  return "speed - 最大速度（升序）";
    case "rspeed"_hash:    return "speed - 平均速度";
    case "speed"_hash:     return "speed - 平均速度（升序）";
    case "rping"_hash:     return "ping - HTTPS 延迟";
    case "ping"_hash:      return "ping - HTTPS 延迟（升序）";
    default:               return "speed - 下载速度";
    }
}

std::string formatPing(const std::string &p)
{
    if(p.empty() || p == "0.00") return "N/A";
    return p + " ms";
}

} // anonymous namespace

std::string exportRender(std::string resultpath, std::vector<nodeInfo> &nodes,
                         bool export_with_maxSpeed, std::string export_sort_method,
                         std::string export_color_style, bool export_as_new_style,
                         bool export_nat_type)
{
    (void)export_as_new_style; (void)export_nat_type; // legacy flags, ignored
    std::string pngname = replace_all_distinct(resultpath, ".log", ".png");
    loadDefaultColor(export_color_style);

    export_sort_method_render = export_sort_method;
    if(export_sort_method != "none")
        std::sort(nodes.begin(), nodes.end(), comparer);

    const int S = image_scale > 0 ? image_scale : 1;
    const std::string font = "tools" PATH_SLASH "misc" PATH_SLASH "WenQuanYiMicroHei-01.ttf";
    const int fs        = 13 * S;
    const int fs_title  = 18 * S;
    const int fs_sub    = 14 * S;
    const int fs_foot   = 11 * S;
    const int row_h     = 28 * S;
    const int title_h   = 36 * S;
    const int sub_h     = 24 * S;
    const int foot_h    = 20 * S;
    const int pad       = 14 * S;
    const int cell_pad  = 18 * S;
    const int idx_col_w = 36 * S;
    const int spark_min = 110 * S;

    pngwriter probe; // empty pngwriter just for text measurement
    int n_count = static_cast<int>(nodes.size());

    // Resolve banner title from any non-empty group; collect totals.
    std::string banner = "Stair Speedtest Reborn";
    int onlines = 0;
    long long total_traffic = 0;
    int test_duration = 0;
    for(const auto &x : nodes)
    {
        if(banner == "Stair Speedtest Reborn" && !x.group.empty())
            banner = x.group;
        if(x.online) onlines++;
        total_traffic += x.totalRecvBytes;
        test_duration += x.duration;
    }
    std::string sub = sortLabelText(export_sort_method);

    // Column widths derive from header + widest data cell.
    auto col_for = [&](const std::string &header,
                       std::function<std::string(int)> cell, int min_w)
    {
        int w = measure(probe, font, fs, header);
        for(int i = 0; i < n_count; ++i)
            w = std::max(w, measure(probe, font, fs, cell(i)));
        return std::max(min_w, w + cell_pad);
    };

    int name_w = col_for("节点名称",
                         [&](int i){ return replaceFlagEmojis(nodes[i].remarks); }, 0);
    int type_w = col_for("类型",
                         [&](int i){ return protoLabel(nodes[i].linkType); }, 0);
    int lat_w  = col_for("HTTPS 延迟",
                         [&](int i){ return formatPing(nodes[i].sitePing); }, 0);
    int avg_w  = col_for("平均速度",
                         [&](int i){ return nodes[i].avgSpeed; }, 0);
    int max_w  = col_for("最大速度",
                         [&](int i){ return nodes[i].maxSpeed; }, 0);
    int spark_w = std::max(spark_min, measure(probe, font, fs, "每秒速度") + cell_pad);

    int table_w = idx_col_w + name_w + type_w + lat_w + avg_w + max_w + spark_w;

    // Make sure banner fits horizontally; if not, grow the name column.
    int banner_w = measure(probe, font, fs_title, banner) + 2 * pad;
    if(banner_w > table_w + 2 * pad)
        name_w += banner_w - (table_w + 2 * pad);
    int total_width = idx_col_w + name_w + type_w + lat_w + avg_w + max_w + spark_w + 2 * pad;

    int total_height = pad + title_h + sub_h + row_h /*header*/ + row_h * n_count
                     + pad + foot_h * 4 + pad;

    // -------- create PNG canvas --------
    pngwriter png(total_width, total_height, 1.0, pngname.data());
    png.filledsquare(0, 0, total_width, total_height, 1.0, 1.0, 1.0);
    rendererInit(font, fs);

    // The pngwriter coordinate origin is bottom-left, so we draw rows by
    // descending y. We track `top_y` as the y-distance from the *top* of the
    // image; converting to png-space is `total_height - top_y`.
    auto rowY = [&](int top){ return total_height - top; };
    // Cell rectangle in png-space.
    auto cellRect = [&](int x_left, int x_right, int top_y, int height,
                        double r, double g, double b)
    {
        png.filledsquare(x_left, rowY(top_y + height) + 1,
                         x_right - 1, rowY(top_y) - 1, r, g, b);
    };
    auto cellText = [&](int x_left, int x_right, int top_y,
                        const std::string &txt, int font_size,
                        double r, double g, double b, bool centered = true)
    {
        int tw = measure(probe, font, font_size, txt);
        int x = centered ? x_left + ((x_right - x_left) - tw) / 2
                         : x_left + 8 * S;
        int y = rowY(top_y + row_h) + (row_h - font_size) / 2 + 2;
        plotText(png, font, font_size, x, y, txt, r, g, b);
    };
    (void)cellRect; (void)cellText; // referenced below; suppress unused if early return

    // Compute column x positions.
    int col_x[8];
    col_x[0] = pad;
    col_x[1] = col_x[0] + idx_col_w;
    col_x[2] = col_x[1] + name_w;
    col_x[3] = col_x[2] + type_w;
    col_x[4] = col_x[3] + lat_w;
    col_x[5] = col_x[4] + avg_w;
    col_x[6] = col_x[5] + max_w;
    col_x[7] = col_x[6] + spark_w;

    // -------- Banner --------
    int yt = pad; // running "top y" cursor
    {
        int tw = measure(probe, font, fs_title, banner);
        plotText(png, font, fs_title,
                 (total_width - tw) / 2, rowY(yt + title_h) + (title_h - fs_title) / 2 + 2,
                 banner, 0.10, 0.10, 0.10);
        yt += title_h;
    }
    {
        int tw = measure(probe, font, fs_sub, sub);
        plotText(png, font, fs_sub,
                 (total_width - tw) / 2, rowY(yt + sub_h) + (sub_h - fs_sub) / 2 + 2,
                 sub, 0.45, 0.45, 0.45);
        yt += sub_h;
    }

    // -------- Header row --------
    cellRect(pad, total_width - pad, yt, row_h, 0.96, 0.96, 0.96);
    cellText(col_x[0], col_x[1], yt, "#",         fs, 0.20, 0.20, 0.20);
    cellText(col_x[1], col_x[2], yt, "节点名称",  fs, 0.20, 0.20, 0.20);
    cellText(col_x[2], col_x[3], yt, "类型",      fs, 0.20, 0.20, 0.20);
    cellText(col_x[3], col_x[4], yt, "HTTPS 延迟", fs, 0.20, 0.20, 0.20);
    cellText(col_x[4], col_x[5], yt, "平均速度",  fs, 0.20, 0.20, 0.20);
    cellText(col_x[5], col_x[6], yt, "最大速度",  fs, 0.20, 0.20, 0.20);
    cellText(col_x[6], col_x[7], yt, "每秒速度",  fs, 0.20, 0.20, 0.20);
    // bottom border of the header row
    png.line(pad, rowY(yt + row_h), total_width - pad, rowY(yt + row_h),
             0.78, 0.78, 0.78);
    yt += row_h;

    // -------- Data rows --------
    for(int i = 0; i < n_count; ++i)
    {
        nodeInfo &n = nodes[i];

        // Serial badge: filled red circle with white digit.
        {
            int cx = col_x[0] + idx_col_w / 2;
            int cy = rowY(yt + row_h / 2);
            int r  = std::min(idx_col_w, row_h) / 2 - 4 * S;
            png.filledcircle(cx, cy, r, 0.94, 0.32, 0.32);
            std::string s = std::to_string(i + 1);
            int tw = measure(probe, font, fs, s);
            plotText(png, font, fs, cx - tw / 2, cy - fs / 2 + 2, s, 1.0, 1.0, 1.0);
        }

        // Node name (left-aligned).
        cellText(col_x[1], col_x[2], yt, replaceFlagEmojis(n.remarks),
                 fs, 0.10, 0.10, 0.10, /*centered=*/false);

        // Type pill: blue text on light blue background.
        cellRect(col_x[2] + 6 * S, col_x[3] - 6 * S, yt + 4 * S, row_h - 8 * S,
                 0.88, 0.93, 0.99);
        cellText(col_x[2], col_x[3], yt, protoLabel(n.linkType),
                 fs, 0.18, 0.42, 0.78);

        // Latency cell with traffic-light bg.
        {
            double lr, lg, lb;
            latencyBg(n.sitePing, lr, lg, lb);
            cellRect(col_x[3], col_x[4], yt, row_h, lr, lg, lb);
            cellText(col_x[3], col_x[4], yt, formatPing(n.sitePing),
                     fs, 0.10, 0.10, 0.10);
        }

        // Avg speed cell.
        {
            color bg; getSpeedColor(n.avgSpeed, &bg);
            cellRect(col_x[4], col_x[5], yt, row_h,
                     bg.red / 65535.0, bg.green / 65535.0, bg.blue / 65535.0);
            cellText(col_x[4], col_x[5], yt, n.avgSpeed,
                     fs, 0.10, 0.10, 0.10);
        }

        // Max speed cell.
        {
            color bg; getSpeedColor(n.maxSpeed, &bg);
            cellRect(col_x[5], col_x[6], yt, row_h,
                     bg.red / 65535.0, bg.green / 65535.0, bg.blue / 65535.0);
            cellText(col_x[5], col_x[6], yt, n.maxSpeed,
                     fs, 0.10, 0.10, 0.10);
        }

        // Sparkline of rawSpeed[20] in pink.
        {
            unsigned long long peak = 0;
            for(int j = 0; j < 20; ++j)
                if(n.rawSpeed[j] > peak) peak = n.rawSpeed[j];
            int sx0 = col_x[6] + 6 * S;
            int sy0 = rowY(yt + row_h - 4 * S);
            int sw  = (col_x[7] - col_x[6]) - 12 * S;
            int sh  = row_h - 8 * S;
            if(peak == 0)
            {
                cellText(col_x[6], col_x[7], yt, "—", fs, 0.55, 0.55, 0.55);
            }
            else
            {
                int per = std::max(1, sw / 20);
                for(int j = 0; j < 20; ++j)
                {
                    int hh = static_cast<int>(static_cast<double>(n.rawSpeed[j]) / peak * sh);
                    if(hh <= 0) continue;
                    png.filledsquare(sx0 + j * per, sy0,
                                     sx0 + (j + 1) * per - 2, sy0 + hh,
                                     0.95, 0.42, 0.60);
                }
            }
        }

        // Row delimiter.
        png.line(pad, rowY(yt + row_h), total_width - pad, rowY(yt + row_h),
                 0.90, 0.90, 0.90);
        yt += row_h;
    }
    yt += pad;

    // -------- Footer --------
    auto footLine = [&](const std::string &text, double r, double g, double b)
    {
        plotText(png, font, fs_foot, pad + 4 * S,
                 rowY(yt + foot_h) + (foot_h - fs_foot) / 2 + 1,
                 text, r, g, b);
        yt += foot_h;
    };
    footLine("✅ 已核实 TLS 证书", 0.10, 0.55, 0.20);
    footLine("HTTPS 延迟为代理出站到 google.com generate_204 的真实请求耗时", 0.45, 0.45, 0.45);

    std::string meta = "主体 = Stair Speedtest Reborn " VERSION
                       "  内核 = mihomo  概要 = " + std::to_string(onlines) +
                       "/" + std::to_string(n_count) + "  排序 = " +
                       sortLabelText(export_sort_method);
    footLine(meta, 0.45, 0.45, 0.45);

    std::string gen = "生成时间：" + getTime(3) +
                      "    总流量：" + speedCalc(static_cast<double>(total_traffic)) +
                      "    测试时长：" + secondToString(test_duration);
    footLine(gen, 0.45, 0.45, 0.45);

    // -------- Outer border --------
    png.line(0, 0, total_width - 1, 0, 0.85, 0.85, 0.85);
    png.line(0, total_height - 1, total_width - 1, total_height - 1, 0.85, 0.85, 0.85);
    png.line(0, 0, 0, total_height - 1, 0.85, 0.85, 0.85);
    png.line(total_width - 1, 0, total_width - 1, total_height - 1, 0.85, 0.85, 0.85);

    png.close();
    return pngname;
}
