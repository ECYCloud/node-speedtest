//! 从节点备注识别区域指示符国旗，并用随包 TwemojiFlat.ttf（COLR）光栅化。
//! 不依赖预置 PNG 国旗包。

use std::path::Path;
use std::sync::OnceLock;

use image::RgbaImage;
use parking_lot::Mutex;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::shape::ShapeContext;
use swash::zeno::Format;
use swash::FontRef;

struct FontCache {
    data: Vec<u8>,
}

static FONT: OnceLock<Option<FontCache>> = OnceLock::new();
static SCALE_CTX: OnceLock<Mutex<ScaleContext>> = OnceLock::new();
static SHAPE_CTX: OnceLock<Mutex<ShapeContext>> = OnceLock::new();

fn font_cache(work_dir: &Path) -> Option<&'static FontCache> {
    FONT.get_or_init(|| {
        let path = work_dir
            .join("tools")
            .join("misc")
            .join("TwemojiFlat.ttf");
        let data = std::fs::read(path).ok()?;
        FontRef::from_index(&data, 0)?;
        Some(FontCache { data })
    })
    .as_ref()
}

/// 拆出备注开头的国旗（一对区域指示符），返回 (flag 文本, 剩余名称)。
pub fn split_flag(s: &str) -> (Option<String>, String) {
    let b = s.as_bytes();
    if b.len() >= 8
        && b[0] == 0xF0
        && b[1] == 0x9F
        && b[2] == 0x87
        && (0xA6..=0xBF).contains(&b[3])
        && b[4] == 0xF0
        && b[5] == 0x9F
        && b[6] == 0x87
        && (0xA6..=0xBF).contains(&b[7])
    {
        let flag = s[..8].to_string();
        let rest = s[8..].trim_start().to_string();
        (Some(flag), rest)
    } else {
        (None, s.to_string())
    }
}

/// ISO 3166-1 alpha-2 → 区域指示符国旗 emoji。
pub fn code_to_flag(cc: &str) -> Option<String> {
    let b = cc.as_bytes();
    if b.len() != 2 {
        return None;
    }
    let a = b[0].to_ascii_uppercase();
    let c = b[1].to_ascii_uppercase();
    if !(b'A'..=b'Z').contains(&a) || !(b'A'..=b'Z').contains(&c) {
        return None;
    }
    let mut out = String::with_capacity(8);
    for ch in [a, c] {
        let cp = 0x1F1E6u32 + (ch - b'A') as u32;
        out.push(char::from_u32(cp)?);
    }
    Some(out)
}

/// 从备注中文地名推断常见国家/地区码（无 emoji、无 GeoIP 时的兜底）。
fn keyword_country_code(name: &str) -> Option<&'static str> {
    const MAP: &[(&str, &str)] = &[
        ("香港", "HK"),
        ("台湾", "TW"),
        ("台灣", "TW"),
        ("澳门", "MO"),
        ("澳門", "MO"),
        ("日本", "JP"),
        ("韩国", "KR"),
        ("韓國", "KR"),
        ("新加坡", "SG"),
        ("美国", "US"),
        ("美國", "US"),
        ("纽约", "US"),
        ("洛杉", "US"),
        ("洛杉磯", "US"),
        ("洛杉矶", "US"),
        ("硅谷", "US"),
        ("英国", "GB"),
        ("英國", "GB"),
        ("伦敦", "GB"),
        ("倫敦", "GB"),
        ("德国", "DE"),
        ("德國", "DE"),
        ("法国", "FR"),
        ("法國", "FR"),
        ("荷兰", "NL"),
        ("荷蘭", "NL"),
        ("俄罗斯", "RU"),
        ("俄羅斯", "RU"),
        ("澳洲", "AU"),
        ("澳大利亚", "AU"),
        ("加拿大", "CA"),
        ("印度", "IN"),
        ("土耳其", "TR"),
        ("阿联酋", "AE"),
        ("迪拜", "AE"),
        ("巴西", "BR"),
        ("阿根廷", "AR"),
        ("泰国", "TH"),
        ("泰國", "TH"),
        ("越南", "VN"),
        ("菲律宾", "PH"),
        ("菲律賓", "PH"),
        ("马来", "MY"),
        ("馬來", "MY"),
        ("印尼", "ID"),
        ("意大利", "IT"),
        ("西班牙", "ES"),
        ("瑞典", "SE"),
        ("瑞士", "CH"),
        ("波兰", "PL"),
        ("波蘭", "PL"),
        ("乌克兰", "UA"),
        ("烏克蘭", "UA"),
        ("尼日利亚", "NG"),
        ("南非", "ZA"),
        ("墨西哥", "MX"),
        ("智利", "CL"),
        ("爱尔兰", "IE"),
        ("愛爾蘭", "IE"),
        ("以色列", "IL"),
        ("中国", "CN"),
        ("大陸", "CN"),
        ("大陆", "CN"),
        ("上海", "CN"),
        ("北京", "CN"),
        ("广州", "CN"),
        ("深圳", "CN"),
    ];
    for &(k, cc) in MAP {
        if name.contains(k) {
            return Some(cc);
        }
    }
    None
}

/// 解析展示用国旗：备注 emoji → 出站 GeoIP → 入站 GeoIP → 中文地名关键词。
pub fn resolve_flag(
    remarks: &str,
    outbound_cc: &str,
    inbound_cc: &str,
) -> (Option<String>, String) {
    let (emoji, rest) = split_flag(remarks);
    if emoji.is_some() {
        return (emoji, rest);
    }
    if let Some(f) = code_to_flag(outbound_cc) {
        return (Some(f), rest);
    }
    if let Some(f) = code_to_flag(inbound_cc) {
        return (Some(f), rest);
    }
    if let Some(cc) = keyword_country_code(&rest) {
        return (code_to_flag(cc), rest);
    }
    (None, rest)
}

/// 将国旗 emoji 渲染为 RGBA 位图（源字形为正方形，边长约 `px`；调用方再裁成 3:2）。
pub fn render_flag(work_dir: &Path, flag: &str, px: u32) -> Option<RgbaImage> {
    let cache = font_cache(work_dir)?;
    let font = FontRef::from_index(&cache.data, 0)?;
    let shape_ctx = SHAPE_CTX.get_or_init(|| Mutex::new(ShapeContext::new()));
    let scale_ctx = SCALE_CTX.get_or_init(|| Mutex::new(ScaleContext::new()));

    let mut glyphs = Vec::new();
    {
        let mut sc = shape_ctx.lock();
        let mut shaper = sc.builder(font).size(px as f32).build();
        shaper.add_str(flag);
        shaper.shape_with(|cluster| {
            for g in cluster.glyphs {
                glyphs.push(g.id);
            }
        });
    }
    if glyphs.is_empty() {
        return None;
    }

    let gid = glyphs[0];
    let mut ctx = scale_ctx.lock();
    let mut scaler = ctx.builder(font).size(px as f32).hint(false).build();
    let image = Render::new(&[
        Source::ColorOutline(0),
        Source::ColorBitmap(StrikeWith::BestFit),
        Source::Outline,
    ])
    .format(Format::Alpha)
    .render(&mut scaler, gid)?;

    let w = image.placement.width.max(1);
    let h = image.placement.height.max(1);
    let mut rgba = RgbaImage::new(w, h);
    match image.content {
        swash::scale::image::Content::Color => {
            let src = &image.data;
            if src.len() < (w * h * 4) as usize {
                return None;
            }
            for y in 0..h {
                for x in 0..w {
                    let i = ((y * w + x) * 4) as usize;
                    rgba.put_pixel(
                        x,
                        y,
                        image::Rgba([src[i], src[i + 1], src[i + 2], src[i + 3]]),
                    );
                }
            }
        }
        swash::scale::image::Content::Mask => {
            let src = &image.data;
            if src.len() < (w * h) as usize {
                return None;
            }
            for y in 0..h {
                for x in 0..w {
                    let a = src[(y * w + x) as usize];
                    rgba.put_pixel(x, y, image::Rgba([0, 0, 0, a]));
                }
            }
        }
        swash::scale::image::Content::SubpixelMask => return None,
    }
    Some(rgba)
}
