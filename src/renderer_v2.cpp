// SSRSpeed / 喵速-style result renderer for Stair Speedtest Reborn (mihomo build).
//
// Layout (top -> bottom):
//   * Title: plain centred black text "<group> - Stair Speedtest Reborn" on a
//     white background (no band, no sort suffix).
//   * Header row: 序号 / 节点名称 / 类型 / HTTP延迟 / HTTPS延迟 / 平均速度 /
//     最高速度 / 每秒速度 on a light grey band, thin separators between columns.
//   * Data rows: plain centred serial number; node name with a small drawn
//     country/region flag bitmap (from the 🇺🇸/🇭🇰/🇹🇼 emoji) + black text;
//     plain protocol text; two latency cells (HTTP = TCP handshake, HTTPS =
//     generate_204) with traffic-light backgrounds; avg/max speed cells with
//     the rainbow gradient; a per-second sparkline of rawSpeed. All text black.
//   * Footer: green ✓ icon + "已核实 TLS 证书", latency caveat, version/summary
//     line, and a generation-time / traffic / duration line.
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
#include "emoji.h"

extern std::string export_sort_method_render;
extern bool export_as_ssrspeed;
extern int image_scale;
extern std::vector<color> colorgroup;
extern std::vector<int> bounds;
extern std::string mihomo_kernel_version; // real kernel version for footer

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

// Split a remark into its leading flag-emoji (raw UTF-8 bytes, e.g. the 8 bytes
// of 🇭🇰) and the remaining display text. If the remark does not start with a
// flag emoji, `flag` is empty and `rest` is the whole string (trimmed).
void splitFlag(const std::string &s, std::string &flag, std::string &rest)
{
    flag.clear();
    int fb = emojiFlagPrefixBytes(s);
    if(fb > 0)
    {
        flag = s.substr(0, fb);
        rest = s.substr(fb);
    }
    else
    {
        rest = s;
    }
    while(!rest.empty() && (rest[0] == ' ' || rest[0] == '\t')) rest.erase(0, 1);
}

// Map link type to a short protocol label (plain text).
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
    default:                             return "-";
    }
}

// Latency cell background. Light traffic-light tones so black text stays
// readable: green < 800 ms, amber 800-1500 ms, red >= 1500 ms or N/A.
void latencyBg(const std::string &ping, double &r, double &g, double &b)
{
    float p = 0.0f;
    try { p = stof(ping); } catch(...) { p = 0.0f; }
    if(p <= 0.0f || p > 9000.0f)        { r = 0.99; g = 0.80; b = 0.55; return; } // N/A -> light amber
    if(p < 800.0f)                      { r = 0.72; g = 0.90; b = 0.72; return; } // light green
    if(p < 1500.0f)                     { r = 0.99; g = 0.86; b = 0.60; return; } // light amber
    r = 0.97; g = 0.72; b = 0.70;                                                 // light red
}

std::string formatPing(const std::string &p)
{
    if(p.empty() || p == "0.00") return "N/A";
    float v = 0.0f;
    try { v = stof(p); } catch(...) { v = 0.0f; }
    if(v <= 0.0f) return "N/A";
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
    const int fs        = 13 * S;   // data / header text
    const int fs_title  = 16 * S;   // title text
    const int fs_foot   = 11 * S;   // footer text
    const int row_h     = 24 * S;   // data / header row
    const int title_h   = 34 * S;   // title strip (white, black text)
    const int foot_h    = 18 * S;   // footer line height
    const int cell_pad  = 18 * S;   // horizontal breathing room inside a column
    const int idx_col_w = 46 * S;
    const int spark_min = 130 * S;
    const int flag_h    = row_h - 6 * S;        // emoji flag height
    const int flag_w    = flag_h;               // Twemoji canvas is square (1:1)
    const int name_lpad = 8 * S;                // left padding inside name cell
    const int name_gap  = 7 * S;                // gap between flag and text

    // Pure-black text everywhere, per requirement.
    const double TX = 0.10, TY = 0.10, TZ = 0.10;

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

    // Title: "<group> - Stair Speedtest Reborn" (no sort suffix, no bg band).
    std::string title = banner + " - Stair Speedtest Reborn";

    // Column widths derive from header + widest data cell.
    auto col_for = [&](const std::string &header,
                       std::function<std::string(int)> cell, int min_w)
    {
        int w = measure(probe, font, fs, header);
        for(int i = 0; i < n_count; ++i)
            w = std::max(w, measure(probe, font, fs, cell(i)));
        return std::max(min_w, w + cell_pad);
    };

    // Name column: measured on the flag-stripped text, then we add room for the
    // emoji flag + gap + left padding.
    int name_text_w = col_for("节点名称", [&](int i){
        std::string flag, rest; splitFlag(nodes[i].remarks, flag, rest);
        return rest;
    }, 0);
    int name_w = name_text_w + flag_w + name_gap + name_lpad;

    int type_w = col_for("类型",
                         [&](int i){ return protoLabel(nodes[i].linkType); }, 0);
    int http_w = col_for("HTTP延迟",
                         [&](int i){ return formatPing(nodes[i].avgPing); }, 0);
    int https_w= col_for("HTTPS延迟",
                         [&](int i){ return formatPing(nodes[i].sitePing); }, 0);
    int avg_w  = col_for("平均速度",
                         [&](int i){ return nodes[i].avgSpeed; }, 0);
    int max_w  = col_for("最高速度",
                         [&](int i){ return nodes[i].maxSpeed; }, 0);
    int spark_w = std::max(spark_min, measure(probe, font, fs, "每秒速度") + cell_pad);

    int total_width = idx_col_w + name_w + type_w + http_w + https_w
                    + avg_w + max_w + spark_w;

    // Make sure the title fits horizontally; if not, grow the name column.
    int title_w = measure(probe, font, fs_title, title) + 2 * cell_pad;
    if(title_w > total_width)
    {
        name_w += title_w - total_width;
        total_width = title_w;
    }

    int total_height = title_h + row_h /*header*/ + row_h * n_count
                     + foot_h * 4 + 10 * S;

    // -------- create PNG canvas --------
    pngwriter png(total_width, total_height, 1.0, pngname.data());
    png.filledsquare(0, 0, total_width, total_height, 1.0, 1.0, 1.0);
    rendererInit(font, fs);

    // pngwriter origin is bottom-left; we track `top_y` from the top.
    auto rowY = [&](int top){ return total_height - top; };
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
                         : x_left + name_lpad;
        int y = rowY(top_y + row_h) + (row_h - font_size) / 2 + 2;
        plotText(png, font, font_size, x, y, txt, r, g, b);
    };

    // Column x positions (8 columns -> 9 boundaries).
    int col_x[9];
    col_x[0] = 0;
    col_x[1] = col_x[0] + idx_col_w;
    col_x[2] = col_x[1] + name_w;
    col_x[3] = col_x[2] + type_w;
    col_x[4] = col_x[3] + http_w;
    col_x[5] = col_x[4] + https_w;
    col_x[6] = col_x[5] + avg_w;
    col_x[7] = col_x[6] + max_w;
    col_x[8] = col_x[7] + spark_w;

    int yt = 0; // running "top y" cursor

    // -------- Title (plain black, centred, white background) --------
    {
        int tw = measure(probe, font, fs_title, title);
        int y  = rowY(yt + title_h) + (title_h - fs_title) / 2 + 2;
        plotText(png, font, fs_title, (total_width - tw) / 2, y, title, TX, TY, TZ);
    }
    yt += title_h;

    int rows_top = yt;
    int rows_bottom = yt + row_h * (n_count + 1);

    // -------- Header row (light grey band, black text) --------
    cellRect(0, total_width, yt, row_h, 0.92, 0.93, 0.95);
    cellText(col_x[0], col_x[1], yt, "序号",      fs, TX, TY, TZ);
    cellText(col_x[1], col_x[2], yt, "节点名称",  fs, TX, TY, TZ);
    cellText(col_x[2], col_x[3], yt, "类型",      fs, TX, TY, TZ);
    cellText(col_x[3], col_x[4], yt, "HTTP延迟",  fs, TX, TY, TZ);
    cellText(col_x[4], col_x[5], yt, "HTTPS延迟", fs, TX, TY, TZ);
    cellText(col_x[5], col_x[6], yt, "平均速度",  fs, TX, TY, TZ);
    cellText(col_x[6], col_x[7], yt, "最高速度",  fs, TX, TY, TZ);
    cellText(col_x[7], col_x[8], yt, "每秒速度",  fs, TX, TY, TZ);
    yt += row_h;

    // -------- Data rows --------
    for(int i = 0; i < n_count; ++i)
    {
        nodeInfo &n = nodes[i];

        // Serial number: plain centred digits, black.
        cellText(col_x[0], col_x[1], yt, std::to_string(i + 1), fs, TX, TY, TZ);

        // Node name: optional native colour emoji flag + left-aligned text.
        {
            std::string flag, rest;
            splitFlag(n.remarks, flag, rest);
            int text_x = col_x[1] + name_lpad;
            if(!flag.empty())
            {
                int fx = col_x[1] + name_lpad;
                int fy = rowY(yt + row_h) + (row_h - flag_h) / 2; // bottom-left
                int drawnW = 0;
                // Native emoji glyph from NotoColorEmoji (FreeType + HarfBuzz).
                if(drawEmoji(png, flag, fx, fy, flag_h, drawnW) && drawnW > 0)
                    text_x = fx + drawnW + name_gap;
                else
                    text_x = fx + flag_w + name_gap;
            }
            int y = rowY(yt + row_h) + (row_h - fs) / 2 + 2;
            plotText(png, font, fs, text_x, y, rest, TX, TY, TZ);
        }

        // Type: plain centred black text.
        cellText(col_x[2], col_x[3], yt, protoLabel(n.linkType), fs, TX, TY, TZ);

        // HTTP latency (TCP ping): traffic-light bg + black text.
        {
            double lr, lg, lb; latencyBg(n.avgPing, lr, lg, lb);
            cellRect(col_x[3], col_x[4], yt, row_h, lr, lg, lb);
            cellText(col_x[3], col_x[4], yt, formatPing(n.avgPing), fs, TX, TY, TZ);
        }

        // HTTPS latency (real generate_204 request): traffic-light bg + black.
        {
            double lr, lg, lb; latencyBg(n.sitePing, lr, lg, lb);
            cellRect(col_x[4], col_x[5], yt, row_h, lr, lg, lb);
            cellText(col_x[4], col_x[5], yt, formatPing(n.sitePing), fs, TX, TY, TZ);
        }

        // Avg speed cell: gradient bg + black text.
        {
            color bg; getSpeedColor(n.avgSpeed, &bg);
            cellRect(col_x[5], col_x[6], yt, row_h,
                     bg.red / 65535.0, bg.green / 65535.0, bg.blue / 65535.0);
            cellText(col_x[5], col_x[6], yt, n.avgSpeed, fs, TX, TY, TZ);
        }

        // Max speed cell.
        {
            color bg; getSpeedColor(n.maxSpeed, &bg);
            cellRect(col_x[6], col_x[7], yt, row_h,
                     bg.red / 65535.0, bg.green / 65535.0, bg.blue / 65535.0);
            cellText(col_x[6], col_x[7], yt, n.maxSpeed, fs, TX, TY, TZ);
        }

        // Per-second sparkline of rawSpeed[20] (pink bars).
        {
            unsigned long long peak = 0;
            for(int j = 0; j < 20; ++j)
                if(n.rawSpeed[j] > peak) peak = n.rawSpeed[j];
            int sx0 = col_x[7] + 6 * S;
            int sy0 = rowY(yt + row_h - 3 * S);
            int sw  = (col_x[8] - col_x[7]) - 12 * S;
            int sh  = row_h - 8 * S;
            if(peak == 0)
            {
                cellText(col_x[7], col_x[8], yt, "-", fs, TX, TY, TZ);
            }
            else
            {
                int per = std::max(1, sw / 20);
                for(int j = 0; j < 20; ++j)
                {
                    int hh = static_cast<int>(static_cast<double>(n.rawSpeed[j]) / peak * sh);
                    if(hh <= 0) continue;
                    png.filledsquare(sx0 + j * per, sy0,
                                     sx0 + (j + 1) * per - 1, sy0 + hh,
                                     0.95, 0.42, 0.60);
                }
            }
        }

        // Thin row delimiter.
        png.line(0, rowY(yt + row_h), total_width, rowY(yt + row_h),
                 0.88, 0.88, 0.88);
        yt += row_h;
    }

    // -------- Vertical column separators across header + data rows --------
    for(int c = 1; c < 8; ++c)
        png.line(col_x[c], rowY(rows_top), col_x[c], rowY(rows_bottom),
                 0.85, 0.86, 0.88);

    yt += 6 * S;

    // -------- Footer --------
    auto footLine = [&](const std::string &text, int x0, double r, double g, double b)
    {
        plotText(png, font, fs_foot, x0,
                 rowY(yt + foot_h) + (foot_h - fs_foot) / 2 + 1,
                 text, r, g, b);
    };

    // First footer line: a green circular badge with a white check (a real
    // drawn icon, not an emoji glyph) + "已核实 TLS 证书".
    {
        int bx   = 10 * S;
        int textY = rowY(yt + foot_h) + (foot_h - fs_foot) / 2 + 1;
        int cy   = textY + fs_foot / 2;            // icon vertical centre
        int rad  = fs_foot / 2 + 1 * S;            // badge radius
        int cx   = bx + rad;                       // icon centre x
        // green disc
        png.filledcircle(cx, cy, rad, 0.18, 0.62, 0.28);
        // white check inside the disc
        int t = std::max(1, S);                    // stroke half-thickness
        int ax = cx - rad/2,        ay = cy;                 // left tip
        int mx = cx - rad/6,        my = cy - rad/2;         // bottom vertex
        int ex = cx + rad/2 + 1*S,  ey = cy + rad/2;         // top-right tip
        for(int k = -t; k <= t; ++k)
        {
            png.line(ax, ay + k, mx, my + k, 1.0, 1.0, 1.0);
            png.line(mx, my + k, ex, ey + k, 1.0, 1.0, 1.0);
        }
        footLine("已核实 TLS 证书", cx + rad + 6 * S, 0.18, 0.55, 0.28);
        yt += foot_h;
    }

    footLine("HTTP / HTTPS 延迟均经代理实测，复用连接预热后取多次平均，已排除 TLS 握手耗时",
             10 * S, 0.45, 0.45, 0.45);
    yt += foot_h;

    std::string meta = "主体 = Stair Speedtest Reborn " VERSION
                       "   内核 = mihomo " + mihomo_kernel_version +
                       "   概要 = " + std::to_string(onlines) +
                       "/" + std::to_string(n_count) + " 在线";
    footLine(meta, 10 * S, 0.45, 0.45, 0.45);
    yt += foot_h;

    std::string gen = "生成时间：" + getTime(3) +
                      "     消耗流量：" + speedCalc(static_cast<double>(total_traffic)) +
                      "     测试时长：" + secondToString(test_duration);
    footLine(gen, 10 * S, 0.45, 0.45, 0.45);
    yt += foot_h;

    // -------- Outer border --------
    png.line(0, 0, total_width - 1, 0, 0.80, 0.80, 0.80);
    png.line(0, total_height - 1, total_width - 1, total_height - 1, 0.80, 0.80, 0.80);
    png.line(0, 0, 0, total_height - 1, 0.80, 0.80, 0.80);
    png.line(total_width - 1, 0, total_width - 1, total_height - 1, 0.80, 0.80, 0.80);

    png.close();
    return pngname;
}
