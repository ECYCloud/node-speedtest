#include <algorithm>
#include <chrono>

#include <pngwriter.h>
#include <zlib.h>
#include <zconf.h>

#include "renderer.h"
#include "version.h"
#include "nodeinfo.h"
#include "string_hash.h"
#include "misc.h"

using namespace std::chrono;

std::string export_sort_method_render = "none";
bool export_as_ssrspeed = false;
// Resolution multiplier applied to every geometric size (font, line height,
// padding, etc). The final image is image_scale * image_scale times denser,
// which makes the result PNG considerably sharper at the cost of file size.
// Externally configured via the [export] image_scale ini key in pref.ini.
int image_scale = 2;

std::vector<color> colorgroup;
std::vector<int> bounds;

//renderer values
int widNumber = 0, widNA = 0, widB = 0, widKB = 0, widMB = 0, widGB = 0, widPercent = 0, widDot = 0;
int widUnkn = 0, widBlock = 0, widFullCone = 0, widRestCone = 0, widPortRest = 0, widSym = 0;

//original color
const int def_colorgroup[5][3] = {{65535, 65535, 65535}, {32768, 65535, 0}, {65535, 65535, 0}, {65535, 32768, 49152}, {65535, 0, 0}};
const int def_bounds[5] = {0, 64 * 1024, 512 * 1024, 4 * 1024 * 1024, 16 * 1024 * 1024};

//rainbow color
const int rainbow_colorgroup[8][3] = {{65535, 65535, 65535}, {26112, 65535, 26112}, {65535, 65535, 26112}, {65535, 45568, 26112}, {65535, 26112, 26112}, {57856, 35840, 65535}, {26112, 52224, 65535}, {26112, 26112, 65535}};
const int rainbow_bounds[8] = {0, 64 * 1024, 512 * 1024, 4 * 1024 * 1024, 16 * 1024 * 1024, 24 * 1024 * 1024, 32 * 1024 * 1024, 40 * 1024 * 1024};

int calcLength(const std::string &data)
{
    int total = 0;
    for(unsigned int i = 0; i < data.size(); i++)
    {
        if(int(data[i]) < 0)
            total += 2;
        else
            total++;
    }
    return total;
}

int getTextLength(const std::string &str)
{
    return ((calcLength(str) - str.size()) / 3) * 2 + (str.size() * 2 - calcLength(str)) - count(str.begin(), str.end(), ' ') / 2;
}

/*
static inline int calcCharCount(std::string data, int type)
{
    int uBound, lBound, total = 0;
    switch(type)
    {
    case 0: //number
        uBound = 57;
        lBound = 48;
        break;
    case 3: //percent
        uBound = 37;
        lBound = 37;
        break;
    case 4: //dot
        uBound = 56;
        lBound = 56;
        break;
    default: //all basic chars
        uBound = 255;
        lBound = 0;
        break;
    }

    for(unsigned int i = 0; i < data.size(); i++)
    {
        if(int(data[i]) >= lBound && int(data[i]) <= uBound)
            total++;
    }
    return total;
}
*/

static inline int getWidth(pngwriter *png, const std::string &font, int fontsize, const std::string &text)
{
    return png->get_text_width_utf8(const_cast<char *>(font.data()), fontsize, const_cast<char *>(text.data()));
    //const int widChnChar = 17, widEngChar = 9;
    //return ((calcLength(text) - text.size()) / 3) * widChnChar + ((text.size() * 2 - calcLength(text)) - count(text.begin(), text.end(), ' ') / 2) * widEngChar;
}

void rendererInit(const std::string &font, int fontsize)
{
    pngwriter png;
    writeLog(LOG_TYPE_RENDER, "Start calculating basic string widths for font '" + font + "' at size " + std::to_string(fontsize) + ".");
    widNumber = getWidth(&png, font, fontsize, "1");
    widNA = getWidth(&png, font, fontsize, "N/A");
    widB = getWidth(&png, font, fontsize, "B");
    widKB = getWidth(&png, font, fontsize, "KB");
    widMB = getWidth(&png, font, fontsize, "MB");
    widGB = getWidth(&png, font, fontsize, "GB");
    widPercent = getWidth(&png, font, fontsize, "%");
    widDot = getWidth(&png, font, fontsize, ".");
    widUnkn = getWidth(&png, font, fontsize, "Unknown");
    widBlock = getWidth(&png, font, fontsize, "Blocked");
    widFullCone = getWidth(&png, font, fontsize, "FullCone");
    widRestCone = getWidth(&png, font, fontsize, "RestrictedCone");
    widPortRest = getWidth(&png, font, fontsize, "PortRestrictedCone");
    widSym = getWidth(&png, font, fontsize, "Symmetric");
    writeLog(LOG_TYPE_RENDER, "Calculated basic string widths: Number=" + std::to_string(widNumber) + " N/A=" + std::to_string(widNA) + " KB=" + std::to_string(widKB) \
             + " MB=" + std::to_string(widMB) + " GB=" + std::to_string(widGB) + " Percent=" + std::to_string(widPercent) + " Dot=" + std::to_string(widDot));
}

static inline int getTextWidth(pngwriter *png, const std::string &font, int fontsize, const std::string &text)
{
    int cntNumber = 0, total_width = 0;

    switch(hash_(text))
    {
    case "N/A"_hash:
        return widNA;
    case "Blocked"_hash:
        return widBlock;
    case "Unknown"_hash:
        return widUnkn;
    case "FullCone"_hash:
        return widFullCone;
    case "RestrictedCone"_hash:
        return widRestCone;
    case "PortRestrictedCone"_hash:
        return widPortRest;
    case "Symmetric"_hash:
        return widSym;
    }

    for(unsigned int i = 0; i < text.size(); i++)
    {
        if(int(text[i]) >= 48 && int(text[i]) <= 57)
            cntNumber++;
    }
    total_width = cntNumber * widNumber + widDot;
    //if(strFind(text, "."))
    //total_width += widDot;

    if(strFind(text, "%"))
        total_width += widPercent;
    else
    {
        if(strFind(text, "MB"))
            total_width += widMB;
        else if(strFind(text, "KB"))
            total_width += widKB;
        else if(strFind(text, "GB"))
            total_width += widGB;
        else if(strFind(text, "B"))
            total_width += widB;
    }

    return total_width;
}

/*
static inline int getTextWidth(pngwriter *png, std::string font, int fontsize, std::string text)
{
    return png->get_text_width_utf8(const_cast<char *>(font.data()), fontsize, const_cast<char *>(text.data()));
}
*/
/*
static inline int getTextWidth(pngwriter *png, std::string font, int fontsize, std::string text)
{
    return calcCharCount(text, 0) * widNumber + calcCharCount(text, 1) * widUpperLetter + calcCharCount(text, 2) * widLowerLetter + calcCharCount(text, 3) * widPercent \
    + calcCharCount(text, 4) * widDot + calcCharCount(text, 5) * widSpace;
}
*/

static inline void plot_text_utf8(pngwriter *png, std::string face_path, int fontsize, int x_start, int y_start, double angle, std::string text, double red, double green, double blue)
{
    png->plot_text_utf8(const_cast<char *>(face_path.data()), fontsize, x_start, y_start, angle, const_cast<char *>(text.data()), red, green, blue);
    return;
}

std::string secondToString(int duration)
{
    int intHrs = duration / 3600;
    int intMin = (duration % 3600) / 60;
    int intSec = duration % 60;
    std::string strHrs = intHrs > 9 ? std::to_string(intHrs) : "0" + std::to_string(intHrs);
    std::string strMin = intMin > 9 ? std::to_string(intMin) : "0" + std::to_string(intMin);
    std::string strSec = intSec > 9 ? std::to_string(intSec) : "0" + std::to_string(intSec);
    return strHrs + ":" + strMin + ":" + strSec;
}

int getSpeed(std::string speed)
{
    if(speed.empty() || speed == "N/A")
        return 0;
    int speedval = 0;
    const string_array units = {"B", "KB", "MB", "GB"};
    size_t index = units.size();
    while(index--)
    {
        if(endsWith(speed, units[index]))
        {
            speedval = std::pow(1024, index) * to_number<float>(speed.substr(0, speed.size() - units[index].size()), 0.0);
            break;
        }
    }
    return speedval;
}

bool comparer(nodeInfo &a, nodeInfo &b)
{
    switch(hash_(export_sort_method_render))
    {
    case "speed"_hash:
        return getSpeed(a.avgSpeed) < getSpeed(b.avgSpeed);
    case "rspeed"_hash:
        return getSpeed(a.avgSpeed) > getSpeed(b.avgSpeed);
    case "maxspeed"_hash:
        return getSpeed(a.maxSpeed) < getSpeed(b.maxSpeed);
    case "rmaxspeed"_hash:
        return getSpeed(a.maxSpeed) > getSpeed(b.maxSpeed);
    case "ping"_hash:
        return stof(a.avgPing) < stof(b.avgPing);
    case "rping"_hash:
        return stof(a.avgPing) > stof(b.avgPing);
    default:
        return a.groupID < b.groupID || a.id < b.id;
    }
}

void getColor(color lc, color rc, float level, color *finalcolor)
{
    finalcolor->red = (int)((float)lc.red * (1.0 - level) + (float)rc.red * level);
    finalcolor->green = (int)((float)lc.green * (1.0 - level) + (float)rc.green * level);
    finalcolor->blue = (int)((float)lc.blue * (1.0 - level) + (float)rc.blue * level);
}

color arrayToColor(const int colors[3])
{
    color retcolor;
    retcolor.red = colors[0];
    retcolor.green = colors[1];
    retcolor.blue = colors[2];
    return retcolor;
}

void getSpeedColor(std::string speed, color *finalcolor)
{
    int speedval = getSpeed(speed);
    unsigned int color_count = colorgroup.size();
    for(unsigned int i = 0; i < color_count - 1; i++)
    {
        if(speedval >= bounds[i] && speedval <= bounds[i + 1])
        {
            getColor(colorgroup[i], colorgroup[i + 1], ((float)speedval - (float)bounds[i]) / ((float)bounds[i + 1] - (float)bounds[i]), finalcolor);
            return;
        }
    }
    getColor(colorgroup[color_count - 1], colorgroup[color_count - 1], 1, finalcolor);
    return;
}

void loadDefaultColor(std::string type)
{
    if(type == "rainbow")
    {
        eraseElements(colorgroup);
        eraseElements(bounds);
        for(int i = 0; i < 8; i++)
        {
            colorgroup.push_back(arrayToColor(rainbow_colorgroup[i]));
            bounds.push_back(rainbow_bounds[i]);
        }
    }
    else if(type == "original")
    {
        eraseElements(colorgroup);
        eraseElements(bounds);
        for(int i = 0; i < 5; i++)
        {
            colorgroup.push_back(arrayToColor(def_colorgroup[i]));
            bounds.push_back(def_bounds[i]);
        }
    }
}


// exportRender() now lives in renderer_v2.cpp — the new SSRSpeed-style
// renderer with serial badge, protocol pill, latency traffic-light, dual
// speed cells, and per-second sparkline.
