// 把仓库内的 NSIS 语言文件同步到 Tauri 本地缓存目录,在 tauri build 之前跑。
//
// 背景:Tauri NSIS 安装界面用的是 NSIS 内置 `SimpChinese.nlf` 里的按钮文字
// (^AgreeBtn 默认是"我接受(&I)"),但同页面说明文本写的是"请点击 [我同意(I)]
// 继续安装",上下不一致。Tauri 的 customLanguageFiles 只能覆盖 Tauri 自己的
// LangString,改不了 NLF 内置字段;唯一干净做法是在仓库放修改过的 nlf,
// build 前把它复制到 Tauri 用来打包的缓存路径,让 makensis 编进 setup.exe。
//
// 仅 Windows 平台执行;其他平台 noop(NSIS 只在 Windows 出包)。
const fs = require("fs");
const path = require("path");
const os = require("os");

if (process.platform !== "win32") {
    console.log("[sync-nsis-lang] 非 Windows,跳过");
    process.exit(0);
}

const localAppData = process.env.LOCALAPPDATA;
if (!localAppData) {
    console.log("[sync-nsis-lang] 未设置 LOCALAPPDATA,跳过");
    process.exit(0);
}

const repoNlf = path.resolve(__dirname, "../src-tauri/nsis/SimpChinese.nlf");
const tauriLangDir = path.join(localAppData, "tauri", "NSIS", "Contrib", "Language files");
const targetNlf = path.join(tauriLangDir, "SimpChinese.nlf");

if (!fs.existsSync(repoNlf)) {
    console.error(`[sync-nsis-lang] 仓库内 nlf 不存在: ${repoNlf}`);
    process.exit(1);
}

// Tauri 缓存目录可能尚未生成(用户首次 build 之前)。这种情况下 Tauri 自己
// 会去拉,我们没必要提前建,直接跳过——下次 build 同步即可。
if (!fs.existsSync(tauriLangDir)) {
    console.log(`[sync-nsis-lang] Tauri NSIS 缓存目录尚未生成,跳过: ${tauriLangDir}`);
    process.exit(0);
}

try {
    fs.copyFileSync(repoNlf, targetNlf);
    console.log(`[sync-nsis-lang] 已同步: ${targetNlf}`);
} catch (e) {
    console.error(`[sync-nsis-lang] 同步失败: ${e.message}`);
    process.exit(1);
}
