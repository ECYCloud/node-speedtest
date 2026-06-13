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
// CI runner 第一次 build 时 LOCALAPPDATA\tauri\NSIS 还没建,Tauri 是在
// beforeBundleCommand(本脚本)跑完之后才下载 NSIS 工具链,所以早期版本的脚本
// 在目录不存在时 exit(0) → makensis 用了 NSIS 自带默认 nlf("我接受(&I)" + PgDn 提示),
// 线上发出去的 setup.exe 永远是错的(本地有 cache 看不出来)。
//
// 第一次修复尝试只下载了 nsis-3.zip 解压,但 Tauri bundler 在 makensis 之前会校验
// NSIS_REQUIRED_FILES,只要缺一个文件(尤其 Plugins/x86-unicode/additional/
// nsis_tauri_utils.dll)就 remove_dir_all + 重下整个 NSIS 目录,我们 sync 进去的
// nlf/nsh 被覆盖。所以本脚本必须完整模拟 Tauri 的 NSIS 安装流程:下载 NSIS zip +
// 单独下载 nsis_tauri_utils.dll,让 Tauri 校验通过不重建,sync 才能保留下来。
//
// URL 跟 Tauri bundler crates/tauri-bundler/src/bundle/windows/nsis/mod.rs 中
// NSIS_URL / NSIS_TAURI_UTILS_URL 对齐。Tauri 升级 NSIS 版本时需同步更新这里。
//
// 仅 Windows 平台执行;其他平台 noop(NSIS 只在 Windows 出包)。
const fs = require("fs");
const path = require("path");
const https = require("https");
const { execFileSync } = require("child_process");

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
const tauriRoot = path.join(localAppData, "tauri");
const nsisRoot = path.join(tauriRoot, "NSIS");
const tauriLangDir = path.join(nsisRoot, "Contrib", "Language files");

// Tauri bundler 写死的 NSIS 工具链下载源(crates/tauri-bundler windows/nsis/mod.rs):
//   const NSIS_URL = "https://github.com/tauri-apps/binary-releases/releases/download/nsis-3.11/nsis-3.11.zip"
//   const NSIS_TAURI_UTILS_URL = ".../nsis-tauri-utils/releases/download/nsis_tauri_utils-v0.5.3/nsis_tauri_utils.dll"
// 解压后里层目录是 nsis-3.11,Tauri 会把它 rename 成 NSIS。
const NSIS_URL = "https://github.com/tauri-apps/binary-releases/releases/download/nsis-3.11/nsis-3.11.zip";
const NSIS_INNER_DIR = "nsis-3.11";
const NSIS_TAURI_UTILS_URL = "https://github.com/tauri-apps/nsis-tauri-utils/releases/download/nsis_tauri_utils-v0.5.3/nsis_tauri_utils.dll";
const NSIS_TAURI_UTILS_REL_PATH = path.join("Plugins", "x86-unicode", "additional", "nsis_tauri_utils.dll");

function httpDownload(url, dst, redirects = 5) {
    return new Promise((resolve, reject) => {
        if (redirects < 0) return reject(new Error("重定向次数过多"));
        const req = https.get(url, { headers: { "User-Agent": "stair-speedtest-build" } }, (res) => {
            if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
                res.resume();
                return resolve(httpDownload(res.headers.location, dst, redirects - 1));
            }
            if (res.statusCode !== 200) {
                res.resume();
                return reject(new Error(`HTTP ${res.statusCode} ${url}`));
            }
            const out = fs.createWriteStream(dst);
            res.pipe(out);
            out.on("finish", () => out.close(() => resolve()));
            out.on("error", reject);
        });
        req.on("error", reject);
        req.setTimeout(120_000, () => req.destroy(new Error("下载超时")));
    });
}

async function ensureNsisToolset() {
    const nsisUtilsAbs = path.join(nsisRoot, NSIS_TAURI_UTILS_REL_PATH);
    // Tauri 校验目录完整性 = LangFiles 目录在 + nsis_tauri_utils.dll 在。
    // 缺任一个 Tauri 都会 remove_dir_all 重建,所以两者必须一起齐备。
    if (fs.existsSync(tauriLangDir) && fs.existsSync(nsisUtilsAbs)) return;

    console.log("[sync-nsis-lang] NSIS 目录缺失或不完整,主动下载完整工具链...");
    fs.mkdirSync(tauriRoot, { recursive: true });

    // 1. 下载 + 解压 NSIS 主包(只在 LangFiles 目录缺时做)
    if (!fs.existsSync(tauriLangDir)) {
        const zipPath = path.join(tauriRoot, "nsis.zip");
        await httpDownload(NSIS_URL, zipPath);
        console.log(`[sync-nsis-lang] 已下载 NSIS: ${zipPath} (${fs.statSync(zipPath).size} bytes)`);
        execFileSync(
            "powershell.exe",
            ["-NoProfile", "-NonInteractive", "-Command",
             `Expand-Archive -Path '${zipPath.replace(/'/g, "''")}' -DestinationPath '${tauriRoot.replace(/'/g, "''")}' -Force`],
            { stdio: "inherit" }
        );
        const innerDir = path.join(tauriRoot, NSIS_INNER_DIR);
        if (fs.existsSync(innerDir)) {
            if (fs.existsSync(nsisRoot)) fs.rmSync(nsisRoot, { recursive: true, force: true });
            fs.renameSync(innerDir, nsisRoot);
        }
        fs.unlinkSync(zipPath);
        if (!fs.existsSync(tauriLangDir)) {
            throw new Error(`解压后仍找不到 LangFiles 目录: ${tauriLangDir}`);
        }
        console.log(`[sync-nsis-lang] NSIS 主包就绪: ${nsisRoot}`);
    }

    // 2. 下载 nsis_tauri_utils.dll 到 Plugins/x86-unicode/additional/
    //    Tauri 会校验这个文件的 hash,但版本号写在 URL 里,只要文件存在 Tauri
    //    最坏情况是 hash 不匹配单独重下这一个,不会触发 remove_dir_all 整个 NSIS。
    if (!fs.existsSync(nsisUtilsAbs)) {
        fs.mkdirSync(path.dirname(nsisUtilsAbs), { recursive: true });
        await httpDownload(NSIS_TAURI_UTILS_URL, nsisUtilsAbs);
        console.log(`[sync-nsis-lang] 已下载 nsis_tauri_utils.dll: ${nsisUtilsAbs} (${fs.statSync(nsisUtilsAbs).size} bytes)`);
    }

    console.log(`[sync-nsis-lang] NSIS 工具链就绪(含 tauri utils plugin)`);
}

// 同步清单:仓库文件名 → 同步到 tauriLangDir 下的同名文件
const files = ["SimpChinese.nlf", "SimpChinese.nsh"];

(async () => {
    try {
        await ensureNsisToolset();
    } catch (e) {
        // 下载失败不致命:Tauri 自己也会再尝试下载,本地用户重跑一次即可。
        // 但首次 CI build 仍会回退默认 nlf,显式报警让 build log 暴露问题。
        console.error(`[sync-nsis-lang] 预下载 NSIS 失败(将退化为 Tauri 自行下载): ${e.message}`);
    }
    if (!fs.existsSync(tauriLangDir)) {
        console.log(`[sync-nsis-lang] LangFiles 目录仍不存在,跳过 nlf/nsh 同步: ${tauriLangDir}`);
        return;
    }
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
})();
