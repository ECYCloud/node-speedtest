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
extern std::string g_local_country, g_local_region, g_local_city, g_local_isp;
extern std::string g_test_start_time, g_test_tz_label;

// Forward decls of helpers defined in renderer.cpp itself:
bool comparer(nodeInfo &a, nodeInfo &b);
void getSpeedColor(std::string speed, color *finalcolor);
void loadDefaultColor();
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

// 把 STUN/RFC 3489 给出的 NAT 类型映射为"UDP 支持等级"的规范化中文标签。
// nodeInfo.natType 内部是 ntt.cpp NAT_TYPE_STR 里的英文枚举字符串(FullCone /
// RestrictedCone / PortRestrictedCone / Symmetric / Blocked / Unknown)。
// 锥形等级越严格,P2P 与 UDP 应用穿透成功率越低，这里按 RFC 3489 的语义直译。
std::string udpSupportLevel(const std::string &nat)
{
    if(nat == "FullCone")           return "完全支持";
    if(nat == "RestrictedCone")     return "受限支持";
    if(nat == "PortRestrictedCone") return "端口受限";
    if(nat == "Symmetric")          return "对称受限";
    if(nat == "Blocked")            return "不支持";
    return "未知";
}

// UDP 单元格底色:支持度越高越偏绿，越低越偏红;未知=浅灰。
void udpBg(const std::string &nat, double &r, double &g, double &b)
{
    if(nat == "FullCone")           { r = 0.72; g = 0.90; b = 0.72; return; }
    if(nat == "RestrictedCone")     { r = 0.85; g = 0.94; b = 0.78; return; }
    if(nat == "PortRestrictedCone") { r = 0.99; g = 0.86; b = 0.60; return; }
    if(nat == "Symmetric")          { r = 0.99; g = 0.80; b = 0.55; return; }
    if(nat == "Blocked")            { r = 0.97; g = 0.72; b = 0.70; return; }
    r = 0.92; g = 0.93; b = 0.95; // Unknown - 与表头同色
}

} // anonymous namespace

std::string exportRender(std::string resultpath, std::vector<nodeInfo> &nodes,
                         bool export_with_maxSpeed, std::string export_sort_method,
                         bool export_as_new_style, bool export_nat_type)
{
    (void)export_as_new_style; // legacy flag, ignored
    std::string pngname = replace_all_distinct(resultpath, ".log", ".png");
    // PNGwriter 内部用窄字符 fopen 写文件，含中文的目标名在 Windows 上会乱码。
    // 对策:若目标名含非 ASCII 字节，先渲染到同目录的 ASCII 临时名，close 后再用
    // fileRenameUtf8(MoveFileW)重命名为真实目标名。纯 ASCII 名则直接写。
    std::string render_path = pngname;
    bool png_need_rename = false;
    for(unsigned char c : pngname)
        if(c >= 0x80) { png_need_rename = true; break; }
    if(png_need_rename)
    {
        // 临时名 = 目标名里非 ASCII 字节替换为 '_'，保证纯 ASCII 且与目标同目录
        // (同盘重命名，原子且快)。close 后用 fileRenameUtf8 改回中文目标名。
        render_path.clear();
        render_path.reserve(pngname.size());
        for(char c : pngname)
            render_path += (static_cast<unsigned char>(c) >= 0x80) ? '_' : c;
    }
    loadDefaultColor();

    export_sort_method_render = export_sort_method;
    if(export_sort_method != "none")
        std::sort(nodes.begin(), nodes.end(), comparer);

    const int S = image_scale > 0 ? image_scale : 1;
    // 思源黑体覆盖 GB18030 全字集(生僻字/繁体/日韩假名/符号)，取代字符集偏窄的
    // 文泉驿微米黑 —— 后者缺字时会把节点名/分组名渲染成空白方块。两份字体随包分发。
    const std::string font = "tools" PATH_SLASH "misc" PATH_SLASH "SourceHanSansCN-Medium.otf";
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
    // 同步判定是否进入"仅延迟模式":任一节点拿到了真实速度数据(avgSpeed 既非
    // 默认 "N/A" 也非空)就显示速度三列;全员 N/A 即 pingonly 模式或全节点失败,
    // 两种情况下速度列都没有展示价值，一并隐藏更整洁。
    std::string banner = "Stair Speedtest Reborn";
    int onlines = 0;
    long long total_traffic = 0;
    int test_duration = 0;
    bool show_speed_cols = false;
    // NAT 检测结果 → UDP 支持等级:FutureHelper.get() 仅首次会阻塞等待，这里
    // 提前批量取出缓存到 vector,后续渲染循环直接用 nat_strs[i],不重复调用。
    std::vector<std::string> nat_strs(n_count);
    bool show_udp_col = false;
    // TLS 证书核实聚合:统计参与了 HTTPS 测速且能下结论的节点数量，以及其中
    // 校验通过的数量。footer "已核实 TLS 证书" 那一行据此条件渲染:
    //   - tls_attempted == 0(全 NotApplicable) → 不画该行，不撒谎
    //   - tls_failed > 0 → 红叉 + "部分节点 TLS 证书校验失败 (n/m)"
    //   - 全部 Verified → 绿勾 + "已核实 TLS 证书"
    int tls_attempted = 0, tls_failed = 0;
    for(int i = 0; i < n_count; ++i)
    {
        if(banner == "Stair Speedtest Reborn" && !nodes[i].group.empty())
            banner = nodes[i].group;
        if(nodes[i].online) onlines++;
        total_traffic += nodes[i].totalRecvBytes;
        test_duration += nodes[i].duration;
        if(!show_speed_cols && !nodes[i].avgSpeed.empty() && nodes[i].avgSpeed != "N/A")
            show_speed_cols = true;
        nat_strs[i] = nodes[i].natType.get();
        // export_nat_type=false 时 UDP 列整体隐藏(用户在 pref.ini 显式关了
        // test_nat_type,即便 natType 有值也不画),与"仅延迟模式"同思路。
        if(export_nat_type && !show_udp_col && !nat_strs[i].empty() && nat_strs[i] != "Unknown")
            show_udp_col = true;
        switch(nodes[i].tlsVerified)
        {
        case TlsVerifyState::Verified: ++tls_attempted;                break;
        case TlsVerifyState::Failed:   ++tls_attempted; ++tls_failed;  break;
        default: break;
        }
    }
    (void)export_with_maxSpeed; // legacy ini flag, 当前由数据驱动自动判定

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
                         [&](int i){ return nodes[i].proxy_type.empty() ? std::string("-") : nodes[i].proxy_type; }, 0);
    // 单列延迟 — 已合并 HTTP/HTTPS 两列(2025-10),HTTPS generate_204 足以反映链路质量
    int ping_w = col_for("延迟",
                         [&](int i){ return formatPing(nodes[i].sitePing); }, 0);
    // 仅延迟模式下三列宽度归零,total_width 自动收缩,sparkline/平均/最高速度的
    // 绘制循环也会跳过这些列，不留空白条
    int avg_w   = 0;
    int max_w   = 0;
    int spark_w = 0;
    if(show_speed_cols)
    {
        avg_w   = col_for("平均速度", [&](int i){ return nodes[i].avgSpeed; }, 0);
        max_w   = col_for("最高速度", [&](int i){ return nodes[i].maxSpeed; }, 0);
        spark_w = std::max(spark_min, measure(probe, font, fs, "每秒速度") + cell_pad);
    }
    // UDP 支持列:整批节点全是 Unknown(test_nat_type 关掉 / STUN 全失败)时
    // 列宽归零，布局自动收缩，与"仅延迟模式"隐藏速度列同思路。
    int udp_w = 0;
    if(show_udp_col)
    {
        udp_w = col_for("UDP", [&](int i){ return udpSupportLevel(nat_strs[i]); }, 0);
    }

    int total_width = idx_col_w + name_w + type_w + ping_w
                    + avg_w + max_w + spark_w + udp_w;

    // Make sure the title fits horizontally; if not, grow the name column.
    int title_w = measure(probe, font, fs_title, title) + 2 * cell_pad;
    if(title_w > total_width)
    {
        name_w += title_w - total_width;
        total_width = title_w;
    }

    // 仅延迟模式下速度三列隐藏 → total_width 收缩到只够装 4-5 个窄列,但 footer
    // "主体 = ... 内核 = ... 概要 = ..." / "测试时间 ... 消耗流量 ... 测试时长 ..."
    // 这两行的宽度跟列数无关，常常远比窄表格宽,溢出右边界直接被画布裁掉
    // (用户截图里"测试时长:00:0..." 就是这么被吞的)。
    // 这里把 footer 实际要画的所有行都先测一遍,任意一行超过 total_width 就把
    // 名称列拉宽来兜住。footer 起始 x = 10*S,所以可用宽度是 total_width - 10*S,
    // 再额外留 10*S 右边距避免文字贴到边线。
    {
        std::vector<std::string> foot_lines;
        if(tls_attempted > 0)
        {
            // TLS 行有图标占位,大致估算 4*foot_h 宽,放在文本左边
            std::string tls_text = (tls_failed > 0)
                ? "部分节点 TLS 证书校验失败 (" + std::to_string(tls_failed) + "/"
                  + std::to_string(tls_attempted) + ")"
                : std::string("已核实 TLS 证书");
            foot_lines.push_back(std::string(8, ' ') + tls_text); // 8 个空格 ~= 图标 + 间距
        }
        foot_lines.push_back("延迟为 HTTPS generate_204 实测，复用连接预热后取多次平均，已排除 TLS 握手耗时");
        {
            std::string parts;
            auto join = [&](const std::string &s){
                if(s.empty()) return;
                if(!parts.empty()) parts += " ";
                parts += s;
            };
            join(g_local_country); join(g_local_region); join(g_local_city);
            if(!g_local_isp.empty()) { if(!parts.empty()) parts += " · "; parts += g_local_isp; }
            if(!parts.empty()) foot_lines.push_back("测试机：" + parts);
        }
        foot_lines.push_back("主体 = Stair Speedtest Reborn " VERSION
                             "   内核 = mihomo " + mihomo_kernel_version +
                             "   概要 = " + std::to_string(onlines) +
                             "/" + std::to_string(n_count) + " 在线");
        {
            std::string when = g_test_start_time.empty() ? getTime(3) : g_test_start_time;
            if(!g_test_tz_label.empty()) when += " " + g_test_tz_label;
            foot_lines.push_back("测试时间：" + when +
                                 "     消耗流量：" + speedCalc(static_cast<double>(total_traffic)) +
                                 "     测试时长：" + secondToString(test_duration));
        }
        int max_foot_w = 0;
        for(const auto &line : foot_lines)
        {
            int w = measure(probe, font, fs_foot, line);
            if(w > max_foot_w) max_foot_w = w;
        }
        // 需要的最小总宽 = 左边距 + footer 最长行 + 右边距
        int needed = 10 * S + max_foot_w + 10 * S;
        if(needed > total_width)
        {
            name_w += needed - total_width;
            total_width = needed;
        }
    }

    int total_height = title_h + row_h /*header*/ + row_h * n_count
                     + foot_h * 5 + 10 * S; // 多预留一行给"测试机:..."

    // -------- create PNG canvas --------
    pngwriter png(total_width, total_height, 1.0, render_path.data());
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
    col_x[4] = col_x[3] + ping_w;
    col_x[5] = col_x[4] + avg_w;
    col_x[6] = col_x[5] + max_w;
    col_x[7] = col_x[6] + spark_w;
    col_x[8] = col_x[7] + udp_w;

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

    // 每秒速度柱形图统一基准:取所有节点 rawSpeed 的全局峰值，而不是各自行峰值。
    // 否则平均 1MB/s 与 50MB/s 的两个节点最高柱都顶到行高，行间没有可比性。
    unsigned long long global_peak = 0;
    for(const auto &x : nodes)
        for(int j = 0; j < 20; ++j)
            if(x.rawSpeed[j] > global_peak) global_peak = x.rawSpeed[j];

    // -------- Header row (light grey band, black text) --------
    cellRect(0, total_width, yt, row_h, 0.92, 0.93, 0.95);
    cellText(col_x[0], col_x[1], yt, "序号",      fs, TX, TY, TZ);
    cellText(col_x[1], col_x[2], yt, "节点名称",  fs, TX, TY, TZ);
    cellText(col_x[2], col_x[3], yt, "类型",      fs, TX, TY, TZ);
    cellText(col_x[3], col_x[4], yt, "延迟",      fs, TX, TY, TZ);
    if(show_speed_cols)
    {
        cellText(col_x[4], col_x[5], yt, "平均速度",  fs, TX, TY, TZ);
        cellText(col_x[5], col_x[6], yt, "最高速度",  fs, TX, TY, TZ);
        cellText(col_x[6], col_x[7], yt, "每秒速度",  fs, TX, TY, TZ);
    }
    if(show_udp_col)
    {
        cellText(col_x[7], col_x[8], yt, "UDP", fs, TX, TY, TZ);
    }
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

        // Type: plain centred black text. proxy_type comes from the kernel.
        cellText(col_x[2], col_x[3], yt, n.proxy_type.empty() ? std::string("-") : n.proxy_type, fs, TX, TY, TZ);

        // 延迟(HTTPS generate_204):红绿灯背景 + 黑色文字
        {
            double lr, lg, lb; latencyBg(n.sitePing, lr, lg, lb);
            cellRect(col_x[3], col_x[4], yt, row_h, lr, lg, lb);
            cellText(col_x[3], col_x[4], yt, formatPing(n.sitePing), fs, TX, TY, TZ);
        }

        if(show_speed_cols)
        {
            // Avg speed cell: gradient bg + black text.
            {
                color bg; getSpeedColor(n.avgSpeed, &bg);
                cellRect(col_x[4], col_x[5], yt, row_h,
                         bg.red / 65535.0, bg.green / 65535.0, bg.blue / 65535.0);
                cellText(col_x[4], col_x[5], yt, n.avgSpeed, fs, TX, TY, TZ);
            }

            // Max speed cell.
            {
                color bg; getSpeedColor(n.maxSpeed, &bg);
                cellRect(col_x[5], col_x[6], yt, row_h,
                         bg.red / 65535.0, bg.green / 65535.0, bg.blue / 65535.0);
                cellText(col_x[5], col_x[6], yt, n.maxSpeed, fs, TX, TY, TZ);
            }

            // 每秒速度柱形图(粉色柱):用全局峰值 global_peak 做分母,
            // 这样不同节点行之间的柱高直接可比 —— 速度高的节点柱子真的更高,
            // 而不是被各自行峰值拉成同样的"贴顶"高度。
            {
                int sx0 = col_x[6] + 6 * S;
                int sy0 = rowY(yt + row_h - 3 * S);
                int sw  = (col_x[7] - col_x[6]) - 12 * S;
                int sh  = row_h - 8 * S;
                if(global_peak == 0)
                {
                    cellText(col_x[6], col_x[7], yt, "-", fs, TX, TY, TZ);
                }
                else
                {
                    int per = std::max(1, sw / 20);
                    for(int j = 0; j < 20; ++j)
                    {
                        if(n.rawSpeed[j] == 0) continue;
                        int hh = static_cast<int>(static_cast<double>(n.rawSpeed[j]) / global_peak * sh);
                        // 极小但非零的速度至少留 1 像素，避免完全看不见。
                        if(hh <= 0) hh = 1;
                        png.filledsquare(sx0 + j * per, sy0,
                                         sx0 + (j + 1) * per - 1, sy0 + hh,
                                         0.95, 0.42, 0.60);
                    }
                }
            }
        }

        // UDP 支持等级:STUN(RFC 3489)结果映射为规范化中文标签 + 渐变背景。
        if(show_udp_col)
        {
            const std::string &nat = nat_strs[i];
            double ur, ug, ub; udpBg(nat, ur, ug, ub);
            cellRect(col_x[7], col_x[8], yt, row_h, ur, ug, ub);
            cellText(col_x[7], col_x[8], yt, udpSupportLevel(nat), fs, TX, TY, TZ);
        }

        // Thin row delimiter.
        png.line(0, rowY(yt + row_h), total_width, rowY(yt + row_h),
                 0.88, 0.88, 0.88);
        yt += row_h;
    }

    // -------- Vertical column separators across header + data rows --------
    // 仅延迟模式只画到延迟列前(c=1..3),速度三列不存在，不需要竖线
    int sep_end = 4;
    if(show_speed_cols) sep_end = 7;
    if(show_udp_col)    sep_end = 8;
    for(int c = 1; c < sep_end; ++c)
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

    // First footer line: TLS 证书核实状态。三态条件渲染,**不再硬编码**:
    //   - tls_attempted == 0:本批节点没有任何一个走完了 HTTPS 测速校验路径
    //     (HTTP 测试 / 全部连不通 / pingonly 模式),无法下结论 → 整行不画。
    //   - tls_failed > 0:任一节点证书校验失败 → 红色 X 圆 + 失败统计文案。
    //   - 全部 Verified:绿勾 + "已核实 TLS 证书"。
    auto drawTlsBadge = [&](double r, double g, double b, bool draw_check)
    {
        int bx   = 10 * S;
        int textY = rowY(yt + foot_h) + (foot_h - fs_foot) / 2 + 1;
        int cy   = textY + fs_foot / 2;
        int rad  = fs_foot / 2 + 1 * S;
        int cx   = bx + rad;
        png.filledcircle(cx, cy, rad, r, g, b);
        int t = std::max(1, S);
        if(draw_check)
        {
            int ax = cx - rad/2,        ay = cy;
            int mx = cx - rad/6,        my = cy - rad/2;
            int ex = cx + rad/2 + 1*S,  ey = cy + rad/2;
            for(int k = -t; k <= t; ++k)
            {
                png.line(ax, ay + k, mx, my + k, 1.0, 1.0, 1.0);
                png.line(mx, my + k, ex, ey + k, 1.0, 1.0, 1.0);
            }
        }
        else
        {
            // X 标记:两条对角线表示"校验失败"。
            int hx1 = cx - rad/2, hy1 = cy - rad/2;
            int hx2 = cx + rad/2, hy2 = cy + rad/2;
            int kx1 = cx - rad/2, ky1 = cy + rad/2;
            int kx2 = cx + rad/2, ky2 = cy - rad/2;
            for(int k = -t; k <= t; ++k)
            {
                png.line(hx1, hy1 + k, hx2, hy2 + k, 1.0, 1.0, 1.0);
                png.line(kx1, ky1 + k, kx2, ky2 + k, 1.0, 1.0, 1.0);
            }
        }
        return cx + rad + 6 * S;
    };
    if(tls_attempted > 0)
    {
        if(tls_failed > 0)
        {
            int textX = drawTlsBadge(0.78, 0.20, 0.20, false);
            std::string msg = "部分节点 TLS 证书校验失败 ("
                              + std::to_string(tls_failed) + "/"
                              + std::to_string(tls_attempted) + ")";
            footLine(msg, textX, 0.62, 0.18, 0.18);
        }
        else
        {
            int textX = drawTlsBadge(0.18, 0.62, 0.28, true);
            footLine("已核实 TLS 证书", textX, 0.18, 0.55, 0.28);
        }
        yt += foot_h;
    }

    footLine("延迟为 HTTPS generate_204 实测，复用连接预热后取多次平均，已排除 TLS 握手耗时",
             10 * S, 0.45, 0.45, 0.45);
    yt += foot_h;

    // 测试机出口位置 + 运营商:四个字段拼成中文地址，任一非空就渲染该行。
    {
        std::string parts;
        auto join = [&](const std::string &s){
            if(s.empty()) return;
            if(!parts.empty()) parts += " ";
            parts += s;
        };
        join(g_local_country);
        join(g_local_region);
        join(g_local_city);
        if(!g_local_isp.empty())
        {
            if(!parts.empty()) parts += " · ";
            parts += g_local_isp;
        }
        if(!parts.empty())
        {
            footLine("测试机：" + parts, 10 * S, 0.45, 0.45, 0.45);
            yt += foot_h;
        }
    }

    std::string meta = "主体 = Stair Speedtest Reborn " VERSION
                       "   内核 = mihomo " + mihomo_kernel_version +
                       "   概要 = " + std::to_string(onlines) +
                       "/" + std::to_string(n_count) + " 在线";
    footLine(meta, 10 * S, 0.45, 0.45, 0.45);
    yt += foot_h;

    // 测试时间(开始时间)+ 时区，而不是图片生成的瞬时。
    // 没填时回退到当前时间，确保独立预览程序也有值。
    std::string when = g_test_start_time.empty() ? getTime(3) : g_test_start_time;
    if(!g_test_tz_label.empty())
        when += " " + g_test_tz_label;
    std::string gen = "测试时间：" + when +
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
    // 渲染到 ASCII 临时名的，close 后重命名为真实(含中文)目标名
    if(png_need_rename)
        fileRenameUtf8(render_path, pngname);
    return pngname;
}
