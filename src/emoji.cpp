// Native colour-emoji renderer: FreeType + HarfBuzz over the bundled Twemoji
// (COLR/CPAL) font. Twemoji's flag glyphs are *flat rectangles* (not the
// waving-cloth Noto style). COLR glyphs are layered vector shapes, so we walk
// the layers (FT_Get_Color_Glyph_Layer) and tint each with its CPAL colour,
// compositing into a float RGBA canvas, then blit (scaled) into the result.
// Falls back to a CBDT/sbix BGRA bitmap if the font provides one instead.

#include <pngwriter.h>
#include <string>
#include <vector>
#include <mutex>
#include <cmath>

#include <ft2build.h>
#include FT_FREETYPE_H
#include FT_COLOR_H
#include <hb.h>
#include <hb-ft.h>

#include "emoji.h"

namespace {

const char *kEmojiFontPath = "tools/misc/TwemojiFlat.ttf";
const int   kRenderPx      = 128; // vector render size before downscale

struct EmojiFont
{
    FT_Library lib = nullptr;
    FT_Face    face = nullptr;
    hb_font_t *hb = nullptr;
    bool       bitmapFont = false;
    bool       ok = false;
    EmojiFont()
    {
        if(FT_Init_FreeType(&lib)) return;
        if(FT_New_Face(lib, kEmojiFontPath, 0, &face)) return;
        if(face->num_fixed_sizes > 0) { FT_Select_Size(face, 0); bitmapFont = true; }
        else FT_Set_Pixel_Sizes(face, 0, kRenderPx);
        hb = hb_ft_font_create(face, nullptr);
        ok = (hb != nullptr);
    }
};

EmojiFont &font() { static EmojiFont f; return f; }
std::mutex g_lock;

inline bool isRI(const unsigned char *p)
{ return p[0]==0xF0 && p[1]==0x9F && p[2]==0x87 && p[3]>=0xA6 && p[3]<=0xBF; }

// Render one COLR base glyph into a tightly-packed RGBA canvas (origin = its
// own bbox top-left). Returns false if the glyph has no COLR layers.
bool renderCOLR(FT_Face face, FT_UInt base, std::vector<float> &rgba,
                int &cw, int &ch)
{
    FT_Palette_Data pd;
    if(FT_Palette_Data_Get(face, &pd)) return false;
    FT_Color *palette = nullptr;
    if(FT_Palette_Select(face, 0, &palette) || !palette) return false;

    // Pass 1: bbox over all layers (in 26.6 -> integer pixels after render).
    FT_LayerIterator it; it.p = nullptr;
    FT_UInt lg, ci;
    int xmin=1e9, ymin=1e9, xmax=-1e9, ymax=-1e9; bool any=false;
    FT_Bool more = FT_Get_Color_Glyph_Layer(face, base, &lg, &ci, &it);
    while(more)
    {
        if(!FT_Load_Glyph(face, lg, FT_LOAD_DEFAULT) &&
           !FT_Render_Glyph(face->glyph, FT_RENDER_MODE_NORMAL))
        {
            FT_GlyphSlot g = face->glyph;
            int l=g->bitmap_left, t=g->bitmap_top;
            int w=(int)g->bitmap.width, h=(int)g->bitmap.rows;
            if(w>0&&h>0){ any=true;
                if(l<xmin)xmin=l; if(t>ymax)ymax=t;
                if(l+w>xmax)xmax=l+w; if(t-h<ymin)ymin=t-h; }
        }
        more = FT_Get_Color_Glyph_Layer(face, base, &lg, &ci, &it);
    }
    if(!any) return false;
    cw = xmax-xmin; ch = ymax-ymin;
    if(cw<=0||ch<=0) return false;
    rgba.assign((size_t)cw*ch*4, 0.0f);

    // Pass 2: composite each layer (painter's algorithm, "over").
    it.p = nullptr;
    more = FT_Get_Color_Glyph_Layer(face, base, &lg, &ci, &it);
    while(more)
    {
        if(!FT_Load_Glyph(face, lg, FT_LOAD_DEFAULT) &&
           !FT_Render_Glyph(face->glyph, FT_RENDER_MODE_NORMAL))
        {
            FT_GlyphSlot g = face->glyph;
            FT_Bitmap &bm = g->bitmap;
            float lr=0, lg2=0, lb=0, la=1.0f;
            if(ci != 0xFFFF) // 0xFFFF = use foreground (text) colour
            { lr=palette[ci].red/255.f; lg2=palette[ci].green/255.f;
              lb=palette[ci].blue/255.f; la=palette[ci].alpha/255.f; }
            for(int ry=0; ry<(int)bm.rows; ++ry)
              for(int rx=0; rx<(int)bm.width; ++rx)
              {
                float cov = bm.buffer[ry*bm.pitch+rx]/255.f * la;
                if(cov<=0.003f) continue;
                int cx = g->bitmap_left - xmin + rx;
                int cy = ymax - g->bitmap_top + ry;
                if(cx<0||cy<0||cx>=cw||cy>=ch) continue;
                float *d = &rgba[((size_t)cy*cw+cx)*4];
                float na = cov + d[3]*(1-cov);
                if(na<=0) continue;
                d[0] = (lr*cov + d[0]*d[3]*(1-cov))/na;
                d[1] = (lg2*cov+ d[1]*d[3]*(1-cov))/na;
                d[2] = (lb*cov + d[2]*d[3]*(1-cov))/na;
                d[3] = na;
              }
        }
        more = FT_Get_Color_Glyph_Layer(face, base, &lg, &ci, &it);
    }
    return true;
}

} // namespace

int emojiFlagPrefixBytes(const std::string &s)
{
    if(s.size() < 8) return 0;
    const unsigned char *p = reinterpret_cast<const unsigned char *>(s.data());
    return (isRI(p) && isRI(p+4)) ? 8 : 0;
}

bool drawEmoji(pngwriter &png, const std::string &emojiUtf8,
               int x, int y, int targetH, int &outW)
{
    outW = 0;
    EmojiFont &f = font();
    if(!f.ok) return false;
    std::lock_guard<std::mutex> guard(g_lock);

    hb_buffer_t *buf = hb_buffer_create();
    hb_buffer_add_utf8(buf, emojiUtf8.c_str(), (int)emojiUtf8.size(), 0, -1);
    hb_buffer_set_direction(buf, HB_DIRECTION_LTR);
    hb_buffer_set_script(buf, HB_SCRIPT_COMMON);
    hb_buffer_set_language(buf, hb_language_from_string("en", -1));
    hb_shape(f.hb, buf, nullptr, 0);
    unsigned int n = 0;
    hb_glyph_info_t *info = hb_buffer_get_glyph_infos(buf, &n);
    if(n == 0) { hb_buffer_destroy(buf); return false; }
    FT_UInt gid = info[0].codepoint;
    hb_buffer_destroy(buf);

    std::vector<float> rgba; int cw=0, ch=0;
    if(!renderCOLR(f.face, gid, rgba, cw, ch))
        return false; // (CBDT bitmap fonts would need the old path; Twemoji is COLR)

    double scale = (double)targetH / ch;
    int dw = (int)(cw*scale + 0.5), dh = targetH;
    if(dw < 1) dw = 1;
    for(int dy=0; dy<dh; ++dy)
    {
        int sy = (int)(dy/scale); if(sy>=ch) sy=ch-1;
        int ty = y + (dh-1-dy); // png y is up; canvas row0 = top
        for(int dx=0; dx<dw; ++dx)
        {
            int sx=(int)(dx/scale); if(sx>=cw) sx=cw-1;
            const float *s=&rgba[((size_t)sy*cw+sx)*4];
            float a=s[3]; if(a<=0.003f) continue;
            int tx=x+dx;
            double br=png.read(tx,ty,1)/65535.0, bg=png.read(tx,ty,2)/65535.0,
                   bb=png.read(tx,ty,3)/65535.0;
            png.plot(tx,ty, s[0]*a+br*(1-a), s[1]*a+bg*(1-a), s[2]*a+bb*(1-a));
        }
    }
    outW = dw;
    return true;
}