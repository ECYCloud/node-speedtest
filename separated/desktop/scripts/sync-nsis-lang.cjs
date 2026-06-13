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
// 线上发出去的 setup.exe 永远是错的(本地有 cache 看不出来)。修复策略:目录缺失
// 时主动下载 Tauri 用的 NSIS zip 并解压到位,Tauri 后续看到目录已就绪会跳过下载,
// 我们 copy 进去的 nlf/nsh 就不会被覆盖。
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
//   const NSIS_URL = "https://github.com/tauri-apps/binary-releases/releases/download/nsis-3/nsis-3.zip"
// 解压后里层目录是 nsis-3.08,Tauri 会把它 rename 成 NSIS。
const NSIS_URL = "https://github.com/tauri-apps/binary-releases/releases/download/nsis-3/nsis-3.zip";
const NSIS_INNER_DIR = "nsis-3.08";

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
    if (fs.existsSync(tauriLangDir)) return;
    console.log("[sync-nsis-lang] 目录缺失,主动下载 NSIS 工具链以确保 Tauri 后续不再覆盖...");
    fs.mkdirSync(tauriRoot, { recursive: true });
    const zipPath = path.join(tauriRoot, "nsis-3.zip");
    await httpDownload(NSIS_URL, zipPath);
    console.log(`[sync-nsis-lang] 已下载: ${zipPath} (${fs.statSync(zipPath).size} bytes)`);
    // PowerShell 5+ 内置 Expand-Archive,GitHub Actions windows runner 默认有
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
    console.log(`[sync-nsis-lang] NSIS 工具链就绪: ${nsisRoot}`);
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
