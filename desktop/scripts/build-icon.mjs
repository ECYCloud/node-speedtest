// 把 desktop/scripts/icon-source.svg 渲染为多分辨率 ICO + 全套 PNG
// 用法:
//   node desktop/scripts/build-icon.mjs

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { Resvg } from "@resvg/resvg-js";
import pngToIco from "png-to-ico";

const here = dirname(fileURLToPath(import.meta.url));
const svgPath = resolve(here, "icon-source.svg");
const iconsDir = resolve(here, "..", "src-tauri", "icons");
mkdirSync(iconsDir, { recursive: true });

const svg = readFileSync(svgPath);

/** 把 SVG 渲染成指定尺寸的 PNG buffer */
function renderPng(size) {
  const r = new Resvg(svg, {
    fitTo: { mode: "width", value: size },
    background: "rgba(0,0,0,0)",
  });
  return r.render().asPng();
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

console.log("\n✓ 全部图标已重新生成,直接 npm run tauri build 即可");

