/**
 * 将本地 Material Icons SVG 栅格化为 PNG（供结果图页脚 blit）。
 * 图标源: assets/material-icons/*.svg（Google Material Icons, Apache-2.0）
 * 输出: src-tauri/engine/tools/misc/icons/*.png
 */
import { Resvg } from "@resvg/resvg-js";
import { readFileSync, writeFileSync, mkdirSync, copyFileSync } from "fs";
import { dirname, join, resolve } from "path";
import { fileURLToPath } from "url";

const here = dirname(fileURLToPath(import.meta.url));
const src = resolve(here, "../assets/material-icons");
const dst = resolve(here, "../src-tauri/engine/tools/misc/icons");
mkdirSync(dst, { recursive: true });

for (const name of ["check_circle", "warning", "cancel"]) {
  const svgPath = join(src, `${name}.svg`);
  const svg = readFileSync(svgPath);
  const r = new Resvg(svg, { fitTo: { mode: "width", value: 128 } });
  const png = r.render().asPng();
  writeFileSync(join(dst, `${name}.png`), png);
  copyFileSync(svgPath, join(dst, `${name}.svg`));
  console.log(`✓ ${name}.png (${png.length} bytes)`);
}
copyFileSync(join(src, "NOTICE.txt"), join(dst, "NOTICE.txt"));
console.log(`✓ 输出目录: ${dst}`);
