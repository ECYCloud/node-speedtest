// 把 desktop/ico.png 的纯黑背景 → 透明,图标本身像素不动。
// 用法: node desktop/scripts/strip-bg.mjs
//
// 策略:从 4 个角向内做迭代式洪水填充(用显式栈,避免递归爆栈)。
// 像素 max(R,G,B) <= BG_THRESHOLD 时视为背景并设 alpha=0;
// 其余像素保持原 RGBA 不变。这样不会误伤图标内部的暗色像素,
// 也不会改写边缘抗锯齿,符合"原封不动,仅去背景"的诉求。
//
// 输出: desktop/scripts/icon-source.png (build-icon.mjs 的输入源)
// 之后请运行 `node desktop/scripts/build-icon.mjs` 生成多尺寸图标
// 与前端侧边栏 logo.png。

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { PNG } from "pngjs";

const here = dirname(fileURLToPath(import.meta.url));
const inputPath = resolve(here, "..", "ico.png");
const outIconSource = resolve(here, "icon-source.png");

const BG_THRESHOLD = 18; // max(R,G,B) <= 该阈值视为黑底(原图取样基本是 0~2)

const src = PNG.sync.read(readFileSync(inputPath));
const { width: W, height: H } = src;
console.log(`✓ 读取 ${inputPath} (${W}×${H}, ${src.data.length} bytes)`);

// 输出 RGBA 缓冲;先把不透明 RGB 复制过去,再把背景洪水填充设 alpha=0
const out = Buffer.alloc(W * H * 4);
const hasAlpha = src.data.length === W * H * 4;
for (let i = 0, j = 0; i < W * H; i++) {
    const si = hasAlpha ? i * 4 : i * 3;
    out[j++] = src.data[si];
    out[j++] = src.data[si + 1];
    out[j++] = src.data[si + 2];
    out[j++] = hasAlpha ? src.data[si + 3] : 255;
}

// visited 位图,1 字节/像素,够用
const visited = new Uint8Array(W * H);
const isBg = (r, g, b) => Math.max(r, g, b) <= BG_THRESHOLD;
const stack = []; // 平铺的像素索引

function tryPush(idx) {
    if (idx < 0 || idx >= W * H) return;
    if (visited[idx]) return;
    const o = idx * 4;
    if (!isBg(out[o], out[o + 1], out[o + 2])) return;
    visited[idx] = 1;
    stack.push(idx);
}

// 4 角种子
tryPush(0);
tryPush(W - 1);
tryPush((H - 1) * W);
tryPush(H * W - 1);

let cleared = 0;
while (stack.length) {
    const idx = stack.pop();
    const x = idx % W;
    const y = (idx / W) | 0;
    out[idx * 4 + 3] = 0; // 设为透明
    cleared++;
    if (x > 0) tryPush(idx - 1);
    if (x < W - 1) tryPush(idx + 1);
    if (y > 0) tryPush(idx - W);
    if (y < H - 1) tryPush(idx + W);
}
console.log(`✓ 透明化背景像素: ${cleared} / ${W * H} (${((cleared / (W * H)) * 100).toFixed(2)}%)`);

const png = new PNG({ width: W, height: H });
out.copy(png.data);
const buf = PNG.sync.write(png);

mkdirSync(dirname(outIconSource), { recursive: true });
writeFileSync(outIconSource, buf);
console.log(`✓ 写出 ${outIconSource} (${buf.length} bytes)`);
console.log(`\n下一步: node desktop/scripts/build-icon.mjs`);
