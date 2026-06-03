#ifndef EMOJI_H_INCLUDED
#define EMOJI_H_INCLUDED

#include <string>

class pngwriter;

// Native colour-emoji rendering via FreeType (FT_LOAD_COLOR) + HarfBuzz, using
// the bundled tools/misc/NotoColorEmoji.ttf. This renders the emoji glyph
// itself (e.g. a flag formed from a regional-indicator pair), exactly like a
// browser / Clash Verge — no per-icon hardcoding.

// If `s` begins with a flag emoji (a pair of Unicode regional-indicator
// symbols, 8 UTF-8 bytes), return 8; otherwise 0.
int emojiFlagPrefixBytes(const std::string &s);

// Render the emoji contained in `emojiUtf8` into `png` so that it fits a box of
// height `targetH`, with its bottom-left at (x, y) in pngwriter space (origin
// bottom-left, 1-indexed). Alpha-composites over existing pixels. The drawn
// width is written to `outW`. Returns false if the font/glyph is unavailable.
bool drawEmoji(pngwriter &png, const std::string &emojiUtf8,
               int x, int y, int targetH, int &outW);

#endif // EMOJI_H_INCLUDED
