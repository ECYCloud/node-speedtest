// 把图标源(优先 PNG,回退 SVG)渲染为多分辨率 ICO + Tauri 期望的全套 PNG。
//
// 输入(自动选择,优先级从高到低):
//   1. desktop/scripts/icon-source.png   ← 用户提供的原始位图(任意尺寸,推荐 1024+)
//   2. desktop/scripts/icon-source.svg   ← 矢量回退
//
// 用法:
//   node desktop/scripts/build-icon.mjs

import { readFileSync, writeFileSync, existsSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { Resvg } from "@resvg/resvg-js";
import pngToIco from "png-to-ico";
import { PNG } from "pngjs";

const here = dirname(fileURLToPath(import.meta.url));
const pngPath = resolve(here, "icon-source.png");
const svgPath = resolve(here, "icon-source.svg");
const iconsDir = resolve(here, "..", "src-tauri", "icons");
mkdirSync(iconsDir, { recursive: true });

// 选择源:PNG 优先,SVG 回退
const usePng = existsSync(pngPath);
let srcPng = null; // 解码后的源 PNG(仅 PNG 模式),后面所有目标尺寸都从它缩放
if (usePng) {
  console.log(`✓ 检测到位图源: icon-source.png`);
  srcPng = PNG.sync.read(readFileSync(pngPath));
  console.log(`  原图 ${srcPng.width}×${srcPng.height}`);
  srcPng = autoCropSquare(srcPng);
  console.log(`  裁剪居中后 ${srcPng.width}×${srcPng.height}(去除透明留白,内容最大化)`);
} else if (existsSync(svgPath)) {
  console.log(`✓ 使用矢量源: icon-source.svg`);
} else {
  throw new Error(
    "找不到图标源:请把 PNG 放到 desktop/scripts/icon-source.png " +
      "或保留 desktop/scripts/icon-source.svg"
  );
}

const svgBuf = usePng ? null : readFileSync(svgPath);

// 自动裁掉源图四周的透明留白,把图形内容居中放进一个正方形画布。
// 解决"图标看起来比别的软件小"的问题 —— 根因是源图上下留白过大,内容只占画布
// 一小块。裁剪后内容填满画布(留极小安全边,避免圆角遮罩切到边缘),视觉上显著放大。
function autoCropSquare(src) {
  let minX = src.width, minY = src.height, maxX = -1, maxY = -1;
  for (let y = 0; y < src.height; y++) {
    for (let x = 0; x < src.width; x++) {
      if (src.data[(y * src.width + x) * 4 + 3] > 16) {
        if (x < minX) minX = x;
        if (x > maxX) maxX = x;
        if (y < minY) minY = y;
        if (y > maxY) maxY = y;
      }
    }
  }
  if (maxX < minX || maxY < minY) return src; // 全透明,原样返回
  const cw = maxX - minX + 1;
  const ch = maxY - minY + 1;
  // 正方形边长 = 内容较长边 + 3% 安全边距(两侧各 1.5%)
  const side = Math.ceil(Math.max(cw, ch) * 1.03);
  const offX = Math.floor((side - cw) / 2);
  const offY = Math.floor((side - ch) / 2);
  const out = new PNG({ width: side, height: side });
  out.data.fill(0); // 透明底
  for (let y = 0; y < ch; y++) {
    for (let x = 0; x < cw; x++) {
      const si = ((minY + y) * src.width + (minX + x)) * 4;
      const di = ((offY + y) * side + (offX + x)) * 4;
      out.data[di] = src.data[si];
      out.data[di + 1] = src.data[si + 1];
      out.data[di + 2] = src.data[si + 2];
      out.data[di + 3] = src.data[si + 3];
    }
  }
  return out;
}

/** 缩放到指定边长(正方形输出)的 PNG buffer */
function renderPng(size) {
  if (usePng) {
    // box filter 平均下采样 — 缩小到图标尺寸(32/128/256)质量足够,无需引入 sharp。
    // 当目标 ≥ 源时退化为 nearest neighbor(放大很少出现,源都是 1024+)。
    const out = boxFilter(srcPng, size, size);
    return PNG.sync.write({ width: size, height: size, data: out });
  } else {
    const r = new Resvg(svgBuf, {
      fitTo: { mode: "width", value: size },
      background: "rgba(0,0,0,0)",
    });
    return r.render().asPng();
  }
}

/** Box filter 缩放:把 src 多个像素平均到 dst 一个像素,缩小专用 */
function boxFilter(src, dstW, dstH) {
  const dst = Buffer.alloc(dstW * dstH * 4);
  const sx = src.width / dstW;
  const sy = src.height / dstH;
  for (let y = 0; y < dstH; y++) {
    const sy0 = Math.floor(y * sy);
    const sy1 = Math.min(Math.ceil((y + 1) * sy), src.height);
    for (let x = 0; x < dstW; x++) {
      const sx0 = Math.floor(x * sx);
      const sx1 = Math.min(Math.ceil((x + 1) * sx), src.width);
      let r = 0, g = 0, b = 0, a = 0, n = 0;
      for (let yy = sy0; yy < sy1; yy++) {
        for (let xx = sx0; xx < sx1; xx++) {
          const i = (yy * src.width + xx) * 4;
          r += src.data[i];
          g += src.data[i + 1];
          b += src.data[i + 2];
          a += src.data[i + 3];
          n++;
        }
      }
      const di = (y * dstW + x) * 4;
      dst[di] = Math.round(r / Math.max(n, 1));
      dst[di + 1] = Math.round(g / Math.max(n, 1));
      dst[di + 2] = Math.round(b / Math.max(n, 1));
      dst[di + 3] = Math.round(a / Math.max(n, 1));
    }
  }
  return dst;
}

// ---------- 1. 生成多分辨率 ICO(包含 16/32/48/64/128/256)----------
const icoSizes = [16, 32, 48, 64, 128, 256];
const icoBuffers = icoSizes.map((s) => Buffer.from(renderPng(s)));
const icoData = await pngToIco(icoBuffers);
writeFileSync(resolve(iconsDir, "icon.ico"), icoData);
console.log(`✓ icon.ico  (含 ${icoSizes.join("/")} 全尺寸, ${icoData.length} bytes)`);

// ---------- 2. 生成 Tauri 期望的各尺寸 PNG ----------
const pngTargets = [
  ["32x32.png", 32],
  ["128x128.png", 128],
  ["128x128@2x.png", 256],
  ["icon.png", 1024],
];
for (const [name, size] of pngTargets) {
  const buf = renderPng(size);
  writeFileSync(resolve(iconsDir, name), buf);
  console.log(`✓ ${name.padEnd(18)} (${size}x${size}, ${buf.length} bytes)`);
}

// ---------- 3. 生成前端侧边栏 logo(512x512:侧栏放大显示到 ~112px,高 DPI 下也锐利)----------
const logoSize = 512;
const logoBuf = renderPng(logoSize);
const logoDir = resolve(here, "..", "src", "assets");
mkdirSync(logoDir, { recursive: true });
const logoPath = resolve(logoDir, "logo.png");
writeFileSync(logoPath, logoBuf);
console.log(`✓ src/assets/logo.png  (${logoSize}x${logoSize}, ${logoBuf.length} bytes)`);

// ---------- 4. 生成 NSIS 安装向导顶部 header.bmp(高分辨率 24-bit BMP,白底)----------
// NSIS Modern UI 2 的 header 图控件在 96 DPI 下逻辑尺寸为 150×57,但安装器是
// DPI-aware 的:在 125%/150%/200% 缩放下控件物理像素会同比放大(最大到 300×114)。
// MUI2 默认 BITMAP_STRETCH=FitControl,内部走 GDI StretchBlt 把 BMP 拉伸到控件大小,
// 拉伸用的是最近邻/COLORONCOLOR,放大时极易糊;只有"原图比控件大、做缩小"才清晰。
// 所以这里直接出 4× = 600×228 的高分辨率 BMP,即使 200% DPI 也是缩小采样,锐利度
// 远好于 150×57。本项目通过 installer-hooks.nsh 的 MUI_HEADERIMAGE_RIGHT 把整张
// BMP 移到 header 右侧,所以图标在画布内右对齐、垂直居中即可。
// BMP 24-bit 不支持透明,这里把图标 alpha 合成到白底再写出。
writeHeaderBmp();

console.log("\n✓ 全部图标已重新生成,接下来运行 npm run tauri build");

function writeHeaderBmp() {
  // 4× 高分辨率画布。逻辑尺寸仍是 NSIS 推荐的 150×57,SCALE 提升只影响实际像素密度,
  // 不破坏 MUI2 的 FitControl 拉伸契约(NSIS 只看像素尺寸,不看 DPI 字段)。
  const SCALE = 4;
  const W = 150 * SCALE, H = 57 * SCALE;
  // 图标在 96 DPI 下视觉占 36px(约 header 高度 60%),按 SCALE 放大到 144px。
  // 右边距同样 14px × SCALE = 56px,保持与 Clash Verge Rev 一致的小角标观感。
  const iconSize = 36 * SCALE;
  const marginRight = 14 * SCALE;

  // 用原始 PNG(1254×1254 高质量位图)+ Lanczos3 缩放 → 144×144,
  // 比 SVG 矢量、比 boxFilter 都更锐利。预乘 alpha 处理避免边缘伪色,
  // 最后叠白底输出 24-bit BMP。
  if (!srcPng) {
    throw new Error("writeHeaderBmp 需要 PNG 源(icon-source.png),当前未加载");
  }
  const iconRgba = lanczos3Resize(srcPng, iconSize, iconSize);

  // W×H 白底画布(RGBA),把图标 alpha 合成到右侧、垂直居中
  const canvas = Buffer.alloc(W * H * 4, 0xff);
  const offX = W - iconSize - marginRight;
  const offY = Math.floor((H - iconSize) / 2);
  for (let y = 0; y < iconSize; y++) {
    for (let x = 0; x < iconSize; x++) {
      const si = (y * iconSize + x) * 4;
      const a = iconRgba[si + 3] / 255;
      const di = ((offY + y) * W + (offX + x)) * 4;
      canvas[di]     = Math.round(iconRgba[si]     * a + 255 * (1 - a));
      canvas[di + 1] = Math.round(iconRgba[si + 1] * a + 255 * (1 - a));
      canvas[di + 2] = Math.round(iconRgba[si + 2] * a + 255 * (1 - a));
      canvas[di + 3] = 255;
    }
  }

  // 编码为 24-bit BMP(BGR,行自下而上,每行 padding 到 4 字节倍数)
  const rowBytes = W * 3;
  const rowPadded = (rowBytes + 3) & ~3;
  const padding = rowPadded - rowBytes;
  const pixelDataSize = rowPadded * H;
  const fileSize = 54 + pixelDataSize;
  const bmp = Buffer.alloc(fileSize);

  // BITMAPFILEHEADER
  bmp.write("BM", 0, "ascii");
  bmp.writeUInt32LE(fileSize, 2);
  bmp.writeUInt32LE(0, 6);
  bmp.writeUInt32LE(54, 10);
  // BITMAPINFOHEADER
  bmp.writeUInt32LE(40, 14);
  bmp.writeInt32LE(W, 18);
  bmp.writeInt32LE(H, 22);
  bmp.writeUInt16LE(1, 26);
  bmp.writeUInt16LE(24, 28);
  bmp.writeUInt32LE(0, 30);          // BI_RGB
  bmp.writeUInt32LE(pixelDataSize, 34);
  // 物理 DPI = 72 × SCALE,与像素密度一致;NSIS 不读这字段,这里只是元数据正确性
  const dpi = Math.round(2835 * SCALE);
  bmp.writeUInt32LE(dpi, 38);
  bmp.writeUInt32LE(dpi, 42);
  bmp.writeUInt32LE(0, 46);
  bmp.writeUInt32LE(0, 50);
  // 像素数据(自下而上)
  let p = 54;
  for (let y = H - 1; y >= 0; y--) {
    for (let x = 0; x < W; x++) {
      const i = (y * W + x) * 4;
      bmp[p++] = canvas[i + 2]; // B
      bmp[p++] = canvas[i + 1]; // G
      bmp[p++] = canvas[i];     // R
    }
    for (let k = 0; k < padding; k++) bmp[p++] = 0;
  }

  writeFileSync(resolve(iconsDir, "header.bmp"), bmp);
  console.log(`✓ header.bmp        (${W}x${H} @${SCALE}x, 24-bit, ${bmp.length} bytes)`);
}


/**
 * Lanczos3 高质量缩放(为 header.bmp 用,比 boxFilter 锐利得多)。
 * 输入: pngjs 解码后的 RGBA 源({width, height, data});输出: RGBA Buffer。
 * 预乘 alpha 处理,避免缩放时边缘出现伪色;最后再反预乘还原 straight alpha。
 * 缩到 56×56 这个尺寸,从 1000+ 像素源开始也能快速跑完(单次 < 1s)。
 */
function lanczos3Resize(src, dstW, dstH) {
  const a = 3;
  const sx = src.width / dstW;
  const sy = src.height / dstH;
  // 缩小时滤波核要按缩放比拉伸,否则会产生锯齿
  const fx = Math.max(1, sx);
  const fy = Math.max(1, sy);
  const rx = Math.ceil(a * fx);
  const ry = Math.ceil(a * fy);

  const sinc = (x) => (x === 0 ? 1 : Math.sin(Math.PI * x) / (Math.PI * x));
  const L = (x) => (Math.abs(x) >= a ? 0 : sinc(x) * sinc(x / a));

  const dst = Buffer.alloc(dstW * dstH * 4);
  for (let y = 0; y < dstH; y++) {
    const cy = (y + 0.5) * sy - 0.5;
    const y0 = Math.max(0, Math.floor(cy - ry + 1));
    const y1 = Math.min(src.height - 1, Math.floor(cy + ry));
    for (let x = 0; x < dstW; x++) {
      const cx = (x + 0.5) * sx - 0.5;
      const x0 = Math.max(0, Math.floor(cx - rx + 1));
      const x1 = Math.min(src.width - 1, Math.floor(cx + rx));
      let r = 0, g = 0, b = 0, alpha = 0, w = 0;
      for (let yy = y0; yy <= y1; yy++) {
        const wy = L((yy - cy) / fy);
        if (wy === 0) continue;
        for (let xx = x0; xx <= x1; xx++) {
          const wx = L((xx - cx) / fx);
          const ww = wx * wy;
          if (ww === 0) continue;
          const i = (yy * src.width + xx) * 4;
          const sa = src.data[i + 3] / 255;       // 预乘
          r += src.data[i]     * sa * ww;
          g += src.data[i + 1] * sa * ww;
          b += src.data[i + 2] * sa * ww;
          alpha += src.data[i + 3] * ww;
          w += ww;
        }
      }
      const di = (y * dstW + x) * 4;
      const aOut = w > 0 ? alpha / w : 0;
      dst[di + 3] = Math.round(Math.max(0, Math.min(255, aOut)));
      if (aOut > 0 && w > 0) {
        // 反预乘还原 straight alpha
        const inv = 255 / aOut;
        dst[di]     = Math.round(Math.max(0, Math.min(255, (r / w) * inv)));
        dst[di + 1] = Math.round(Math.max(0, Math.min(255, (g / w) * inv)));
        dst[di + 2] = Math.round(Math.max(0, Math.min(255, (b / w) * inv)));
      } else {
        dst[di] = dst[di + 1] = dst[di + 2] = 0;
      }
    }
  }
  return dst;
}
