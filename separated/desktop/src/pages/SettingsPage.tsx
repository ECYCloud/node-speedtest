import { Button, Card, SectionTitle } from "../components/ui";
import { RefreshCw, Monitor, Download, ExternalLink, CheckCircle2, AlertCircle, PackageCheck, Sparkles, Trash2 } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { check as checkAppUpdater, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { getVersion } from "@tauri-apps/api/app";
import { useState } from "react";

// /checkupdate 接口返回结构(由 webgui_wrapper.cpp 序列化):
//   - local       本地内核版本(mihomo -v 读出)
//   - latest      GitHub /releases/latest 的 tag_name,失败时为空
//   - has_update  latest > local 时为 true
//   - release_url 始终是 GitHub releases 页地址
//   - error       检查失败时的中文说明,成功时为空
type UpdateInfo = {
  local: string;
  latest: string;
  has_update: boolean;
  release_url: string;
  error: string;
};

// download_mihomo_update Tauri 命令的返回结构
type UpdateResult = {
  success: boolean;
  new_version: string;
  error: string;
};

// clear_app_data Tauri 命令的返回结构:cleared 是已删除的路径列表,
// errors 是 best-effort 删除时被占用而跳过的项(用户可见用于排查残留)
type ClearAppDataResult = {
  cleared: string[];
  errors: string[];
};

// 应用自身更新状态机 — 由 tauri-plugin-updater 驱动:
//   idle            未检查
//   checking        正在请求 latest.json
//   none            已是最新
//   available       发现新版,等用户确认下载安装
//   downloading     下载中(received/total 字节,total 可能为 0 表示未知)
//   installing      下载完成,正在调用平台安装器(NSIS passive / .app 替换 / AppImage 覆盖)
//   ready           安装完成,即将 relaunch
//   error           任意阶段失败,带可读错误
type AppUpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "none"; current: string }
  | { kind: "available"; current: string; latest: string; notes: string; update: Update }
  | { kind: "downloading"; current: string; latest: string; received: number; total: number }
  | { kind: "installing"; latest: string }
  | { kind: "ready"; latest: string }
  | { kind: "error"; message: string };

const APP_RELEASE_URL = "https://github.com/ECYCloud/stairspeedtest-reborn-mihomo/releases/latest";

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

export default function SettingsPage() {
  const [restarting, setRestarting] = useState(false);
  const [checking, setChecking] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [installing, setInstalling] = useState(false);
  const [installResult, setInstallResult] = useState<UpdateResult | null>(null);
  const [appUpdate, setAppUpdate] = useState<AppUpdateState>({ kind: "idle" });
  // 二次确认对话框开关:第一次点"清理应用数据"只展开警告,第二次点确认才真正执行
  const [confirmClear, setConfirmClear] = useState(false);
  const [clearing, setClearing] = useState(false);
  const [clearError, setClearError] = useState<string | null>(null);

  async function restart() {
    setRestarting(true);
    try {
      await invoke("restart_backend");
    } finally {
      setRestarting(false);
    }
  }

  // 检查应用自身更新:走 tauri-plugin-updater,读 GitHub release 里的 latest.json,
  // 内部用 minisign 公钥验签;只查不下载,用户确认后再调 installAppUpdate
  async function checkUpdateApp() {
    setAppUpdate({ kind: "checking" });
    try {
      const update = await checkAppUpdater();
      if (!update) {
        // plugin 把当前版本读自 tauri.conf.json,checkAppUpdater 拿不到 update 时只表示"已是最新"
        const current = await getVersion();
        setAppUpdate({ kind: "none", current });
        return;
      }
      setAppUpdate({
        kind: "available",
        current: update.currentVersion,
        latest: update.version,
        notes: update.body ?? "",
        update,
      });
    } catch (e) {
      setAppUpdate({ kind: "error", message: String(e) });
    }
  }

  // 下载并安装:downloadAndInstall 在三平台行为不同
  //   Windows: 下载 .nsis.zip → 解压 setup.exe → 启动 NSIS(passive 模式带进度)
  //   macOS:   下载 .app.tar.gz → 解压替换当前 .app → 提示 relaunch
  //   Linux:   下载 .AppImage → 替换当前 AppImage 文件 → 提示 relaunch
  // 完成后调 relaunch() 让 Tauri 关掉当前进程并重启新版本
  async function installUpdateApp() {
    if (appUpdate.kind !== "available") return;
    const u = appUpdate.update;
    const latest = appUpdate.latest;
    let received = 0;
    let total = 0;
    setAppUpdate({ kind: "downloading", current: appUpdate.current, latest, received: 0, total: 0 });
    try {
      await u.downloadAndInstall((event) => {
        switch (event.event) {
          case "Started":
            total = event.data.contentLength ?? 0;
            setAppUpdate({ kind: "downloading", current: appUpdate.current, latest, received: 0, total });
            break;
          case "Progress":
            received += event.data.chunkLength;
            setAppUpdate({ kind: "downloading", current: appUpdate.current, latest, received, total });
            break;
          case "Finished":
            setAppUpdate({ kind: "installing", latest });
            break;
        }
      });
      setAppUpdate({ kind: "ready", latest });
      // Windows passive NSIS 会自己接管,以下 relaunch 主要服务 macOS/Linux
      await relaunch();
    } catch (e) {
      setAppUpdate({ kind: "error", message: String(e) });
    }
  }

  async function checkUpdate() {
    setChecking(true);
    setInstallResult(null);
    try {
      // 经 Rust 端 .no_proxy() 客户端转发,绕过系统代理对 127.0.0.1 的拦截。
      const text = await invoke<string>("api_get", { path: "/checkupdate" });
      setUpdateInfo(JSON.parse(text) as UpdateInfo);
    } catch (e) {
      setUpdateInfo({
        local: "",
        latest: "",
        has_update: false,
        release_url: "https://github.com/MetaCubeX/mihomo/releases/latest",
        error: String(e),
      });
    } finally {
      setChecking(false);
    }
  }

  async function installUpdate() {
    setInstalling(true);
    try {
      // 后端会先 taskkill 在跑的 mihomo,再写入新 exe;失败自动从 .bak 还原。
      // 下载 16.7 MB 在普通网络下数秒内完成,慢网络可能要 30-60 秒。
      const r = await invoke<UpdateResult>("download_mihomo_update");
      setInstallResult(r);
      if (r.success && updateInfo) {
        // 安装成功后把当前显示的本地版本同步成新装的版本,并把"有更新"清掉。
        // 注意:本地正在运行的 mihomo -v 仍是旧的,要等下次测速才会启新进程。
        // 这里只是 UI 反馈,真实情况看 mihomo 启动日志。
        setUpdateInfo({
          ...updateInfo,
          local: r.new_version,
          has_update: false,
        });
      }
    } catch (e) {
      setInstallResult({
        success: false,
        new_version: "",
        error: String(e),
      });
    } finally {
      setInstalling(false);
    }
  }

  // 用户在二次确认对话框点"确认清理"才会进入这里:
  // 1. 调 clear_app_data 让 Rust 停后端 + 删 engine 目录
  // 2. Rust 端清理完成后会延迟 300ms 自行触发 app.restart() 重启进程,
  //    前端这里只需保持"清理中"状态等被打断即可,不需要也不能调 window.close()
  //    (后者会被 capabilities 默认 ACL 拒掉)
  // 3. 失败把 errors 显示出来,不重启应用,方便排查
  async function clearAppData() {
    setClearing(true);
    setClearError(null);
    try {
      const r = await invoke<ClearAppDataResult>("clear_app_data");
      if (r.errors.length > 0) {
        setClearError(`部分项未能删除：\n${r.errors.join("\n")}`);
        setClearing(false);
        return;
      }
      // 清理成功:进程即将由 Rust 端调度重启,UI 保持禁用态等被打断
    } catch (e) {
      setClearError(String(e));
      setClearing(false);
    }
  }

  return (
    <div className="flex flex-col gap-4">
      <Card className="p-5">
        <SectionTitle desc="后端进程异常或修改 pref.ini 后，可手动重启">
          后端进程
        </SectionTitle>
        <Button onClick={restart} disabled={restarting}>
          <RefreshCw
            size={14}
            className={restarting ? "animate-spin" : ""}
          />
          重启后端
        </Button>
      </Card>

      <Card className="p-5">
        <SectionTitle desc="检查 Stair Speedtest 是否有新版（来源：GitHub ECYCloud/stairspeedtest-reborn-mihomo）">
          软件更新
        </SectionTitle>
        <div className="flex items-center gap-3 flex-wrap">
          <Button onClick={checkUpdateApp} disabled={appUpdate.kind === "checking" || appUpdate.kind === "downloading" || appUpdate.kind === "installing"}>
            <Sparkles size={14} className={appUpdate.kind === "checking" ? "animate-pulse" : ""} />
            {appUpdate.kind === "checking" ? "检查中…" : "检查更新"}
          </Button>
          {(appUpdate.kind === "none" || appUpdate.kind === "available" || appUpdate.kind === "downloading") && (
            <span className="text-sm text-fg-muted">
              本地 <code className="text-xs">v{
                appUpdate.kind === "none" ? appUpdate.current :
                appUpdate.kind === "available" ? appUpdate.current :
                appUpdate.current
              }</code>
              {(appUpdate.kind === "available" || appUpdate.kind === "downloading") && (
                <>
                  <span className="mx-2">·</span>
                  最新 <code className="text-xs">v{appUpdate.latest}</code>
                </>
              )}
            </span>
          )}
        </div>
        {appUpdate.kind !== "idle" && appUpdate.kind !== "checking" && (
          <div className="mt-3 text-sm">
            {appUpdate.kind === "error" ? (
              <div className="flex flex-col gap-2">
                <div className="flex items-start gap-2 text-red-500">
                  <AlertCircle size={14} className="mt-0.5 shrink-0" />
                  <span className="break-all">{appUpdate.message}</span>
                </div>
                <div>
                  <button
                    type="button"
                    className="inline-flex items-center gap-1 text-sm text-blue-500 hover:underline"
                    onClick={() => openUrl(APP_RELEASE_URL)}
                  >
                    <ExternalLink size={14} />
                    前往下载页(手动)
                  </button>
                </div>
              </div>
            ) : appUpdate.kind === "none" ? (
              <div className="flex items-center gap-2 text-green-600">
                <CheckCircle2 size={14} />
                <span>已是最新版本</span>
              </div>
            ) : appUpdate.kind === "available" ? (
              <div className="flex flex-col gap-2">
                <div className="flex items-center gap-2 text-amber-500">
                  <AlertCircle size={14} />
                  <span>发现新版本 v{appUpdate.latest}，可一键下载并自动安装</span>
                </div>
                {appUpdate.notes && (
                  <pre className="whitespace-pre-wrap break-words text-xs text-fg-muted bg-bg-subtle rounded p-2 max-h-32 overflow-auto font-sans">
                    {appUpdate.notes}
                  </pre>
                )}
                <div className="flex items-center gap-3 flex-wrap">
                  <Button onClick={installUpdateApp}>
                    <PackageCheck size={14} />
                    下载并安装
                  </Button>
                  <button
                    type="button"
                    className="inline-flex items-center gap-1 text-sm text-blue-500 hover:underline"
                    onClick={() => openUrl(APP_RELEASE_URL)}
                  >
                    <ExternalLink size={14} />
                    手动下载
                  </button>
                </div>
              </div>
            ) : appUpdate.kind === "downloading" ? (
              <div className="flex flex-col gap-2">
                <div className="flex items-center gap-2 text-amber-500">
                  <Download size={14} className="animate-pulse" />
                  <span>
                    下载中 v{appUpdate.latest} —{" "}
                    {appUpdate.total > 0
                      ? `${formatBytes(appUpdate.received)} / ${formatBytes(appUpdate.total)} (${Math.round((appUpdate.received / appUpdate.total) * 100)}%)`
                      : `已下载 ${formatBytes(appUpdate.received)}`}
                  </span>
                </div>
                {appUpdate.total > 0 && (
                  <div className="h-1.5 w-full bg-bg-subtle rounded overflow-hidden">
                    <div
                      className="h-full bg-amber-500 transition-all"
                      style={{ width: `${Math.min(100, (appUpdate.received / appUpdate.total) * 100)}%` }}
                    />
                  </div>
                )}
              </div>
            ) : appUpdate.kind === "installing" ? (
              <div className="flex items-center gap-2 text-amber-500">
                <PackageCheck size={14} className="animate-pulse" />
                <span>正在安装 v{appUpdate.latest}…</span>
              </div>
            ) : appUpdate.kind === "ready" ? (
              <div className="flex items-center gap-2 text-green-600">
                <CheckCircle2 size={14} />
                <span>已安装 v{appUpdate.latest}，应用即将重启</span>
              </div>
            ) : null}
          </div>
        )}
      </Card>

      <Card className="p-5">
        <SectionTitle desc="检查 mihomo 内核是否有新版可下载（来源：GitHub MetaCubeX/mihomo）">
          mihomo 内核更新
        </SectionTitle>
        <div className="flex items-center gap-3 flex-wrap">
          <Button onClick={checkUpdate} disabled={checking || installing}>
            <Download size={14} className={checking ? "animate-pulse" : ""} />
            {checking ? "检查中…" : "检查更新"}
          </Button>
          {updateInfo && !updateInfo.error && (
            <span className="text-sm text-fg-muted">
              本地 <code className="text-xs">{updateInfo.local || "未知"}</code>
              <span className="mx-2">·</span>
              最新 <code className="text-xs">{updateInfo.latest || "未知"}</code>
            </span>
          )}
        </div>
        {updateInfo && (
          <div className="mt-3 text-sm">
            {updateInfo.error ? (
              <div className="flex items-center gap-2 text-red-500">
                <AlertCircle size={14} />
                <span>{updateInfo.error}</span>
              </div>
            ) : updateInfo.has_update ? (
              <div className="flex flex-col gap-2">
                <div className="flex items-center gap-2 text-amber-500">
                  <AlertCircle size={14} />
                  <span>发现新版本 {updateInfo.latest}，可一键下载并替换本地内核</span>
                </div>
                <div className="flex items-center gap-3 flex-wrap">
                  <Button onClick={installUpdate} disabled={installing}>
                    <PackageCheck size={14} className={installing ? "animate-pulse" : ""} />
                    {installing ? "下载并安装中…" : "下载并安装"}
                  </Button>
                  <button
                    type="button"
                    className="inline-flex items-center gap-1 text-sm text-blue-500 hover:underline disabled:text-fg-muted"
                    disabled={installing}
                    onClick={() => openUrl(updateInfo.release_url)}
                  >
                    <ExternalLink size={14} />
                    手动下载
                  </button>
                </div>
              </div>
            ) : updateInfo.latest ? (
              <div className="flex items-center gap-2 text-green-600">
                <CheckCircle2 size={14} />
                <span>已是最新版本</span>
              </div>
            ) : null}
          </div>
        )}
        {installResult && (
          <div className="mt-3 text-sm">
            {installResult.success ? (
              <div className="flex items-center gap-2 text-green-600">
                <CheckCircle2 size={14} />
                <span>已更新到 {installResult.new_version}，下次测速时自动启用新内核</span>
              </div>
            ) : (
              <div className="flex items-center gap-2 text-red-500">
                <AlertCircle size={14} />
                <span>安装失败：{installResult.error}</span>
              </div>
            )}
          </div>
        )}
      </Card>

      <Card className="p-5">
        <SectionTitle desc="清除测速记录、订阅历史、运行日志、mihomo 缓存与已修改的 pref.ini">
          清理应用数据
        </SectionTitle>
        {!confirmClear ? (
          <Button
            variant="danger"
            onClick={() => {
              setClearError(null);
              setConfirmClear(true);
            }}
          >
            <Trash2 size={14} />
            清理应用数据
          </Button>
        ) : (
          <div className="flex flex-col gap-3">
            <div className="flex items-start gap-2 text-sm text-amber-500">
              <AlertCircle size={14} className="mt-0.5 shrink-0" />
              <span>
                此操作会删除 <code className="text-xs">%LOCALAPPDATA%\com.stairspeedtest.desktop\engine</code>{" "}
                下的全部用户数据（测速结果、订阅、日志、配置、mihomo 缓存），且无法恢复。完成后应用会自动重启，重新同步默认引擎资产。
              </span>
            </div>
            <div className="flex items-center gap-3 flex-wrap">
              <Button variant="danger" onClick={clearAppData} disabled={clearing}>
                <Trash2 size={14} className={clearing ? "animate-pulse" : ""} />
                {clearing ? "清理中…" : "确认清理并重启应用"}
              </Button>
              <Button onClick={() => setConfirmClear(false)} disabled={clearing}>
                取消
              </Button>
            </div>
          </div>
        )}
        {clearError && (
          <div className="mt-3 flex items-start gap-2 text-sm text-red-500">
            <AlertCircle size={14} className="mt-0.5 shrink-0" />
            <pre className="whitespace-pre-wrap break-all font-sans">{clearError}</pre>
          </div>
        )}
      </Card>

      <Card className="p-5">
        <SectionTitle desc="测速参数等高级设置直接编辑 engine/pref.ini">
          高级配置
        </SectionTitle>
        <div className="flex items-center gap-2 text-sm text-fg-muted">
          <Monitor size={14} />
          <code className="text-xs">engine/pref.ini</code>
          <span>·</span>
          <span>修改后点击上方"重启后端"生效</span>
        </div>
      </Card>
    </div>
  );
}
