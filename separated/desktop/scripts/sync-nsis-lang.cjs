// 把仓库内的 NSIS 语言文件同步到 Tauri 本地缓存目录,在 tauri build 之前跑。
//
// 背景:
//   1. SimpChinese.nlf — Tauri NSIS 安装界面用的是 NSIS 内置 nlf 里的按钮文字
//      (^AgreeBtn 默认是"我接受(&I)"),但同页面说明文本写的是"请点击 [我同意(I)]
//      继续安装",上下不一致。Tauri 的 customLanguageFiles 只能覆盖 Tauri 自己的
//      LangString,改不了 NLF 内置字段。
//   2. SimpChinese.nsh — Modern UI 2 的 LangString(MUI_INNERTEXT_LICENSE_TOP /
//      _BOTTOM 等许可证页文案)同样不在 nlf 里,也不接受 customLanguageFiles 覆盖。
//
// 唯一干净做法是在仓库放修改过的版本,build 前把它复制到 Tauri 用来打包的缓存路径,
// 让 makensis 编进 setup.exe。
//
// 仅 Windows 平台执行;其他平台 noop(NSIS 只在 Windows 出包)。
const fs = require("fs");
const path = require("path");

if (process.platform !== "win32") {
    console.log("[sync-nsis-lang] 非 Windows,跳过");
    process.exit(0);
}

const localAppData = process.env.LOCALAPPDATA;
if (!localAppData) {
    console.log("[sync-nsis-lang] 未设置 LOCALAPPDATA,跳过");
    process.exit(0);
}

const repoNsisDir = path.resolve(__dirname, "../src-tauri/nsis");
const tauriLangDir = path.join(localAppData, "tauri", "NSIS", "Contrib", "Language files");

// Tauri 缓存目录可能尚未生成(用户首次 build 之前)。这种情况下 Tauri 自己
// 会去拉,我们没必要提前建,直接跳过——下次 build 同步即可。
if (!fs.existsSync(tauriLangDir)) {
    console.log(`[sync-nsis-lang] Tauri NSIS 缓存目录尚未生成,跳过: ${tauriLangDir}`);
    process.exit(0);
}

// 同步清单:仓库文件名 → 同步到 tauriLangDir 下的同名文件
const files = ["SimpChinese.nlf", "SimpChinese.nsh"];

for (const name of files) {
    const src = path.join(repoNsisDir, name);
    const dst = path.join(tauriLangDir, name);
    if (!fs.existsSync(src)) {
        console.error(`[sync-nsis-lang] 仓库内文件不存在: ${src}`);
        process.exit(1);
    }
    try {
        fs.copyFileSync(src, dst);
        console.log(`[sync-nsis-lang] 已同步: ${dst}`);
    } catch (e) {
        console.error(`[sync-nsis-lang] 同步失败 ${name}: ${e.message}`);
        process.exit(1);
    }
}
