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

console.log("\n✓ 全部图标已重新生成,接下来运行 npm run tauri build");
