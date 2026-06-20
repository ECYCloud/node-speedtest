import { Button, Card, SectionTitle } from "../components/ui";
import { RefreshCw, Monitor, Download, ExternalLink, CheckCircle2, AlertCircle, PackageCheck, Sparkles, Trash2 } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "react";
import { useAppUpdate } from "../store/appUpdate";
import { useMihomoUpdate } from "../store/mihomoUpdate";

// 类型定义已搬到 src/store/mihomoUpdate.ts(MihomoUpdateInfo / MihomoInstallResult)。

// clear_app_data Tauri 命令的返回结构:cleared 是已删除的路径列表,
// errors 是 best-effort 删除时被占用而跳过的项(用户可见用于排查残留)
type ClearAppDataResult = {
  cleared: string[];
  errors: string[];
};

// 应用自更新与 mihomo 内核更新的状态机均搬到 src/store/{appUpdate,mihomoUpdate}.ts:
// 用户离开设置页(组件 unmount)时，局部 useState 会销毁导致进度/检查中态丢失。
// 现在状态由 zustand 持有，跨页面切换无缝继续。

const APP_RELEASE_URL = "https://github.com/ECYCloud/stairspeedtest-reborn-mihomo/releases/latest";

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

export default function SettingsPage() {
  const [restarting, setRestarting] = useState(false);
  // 应用自更新状态由全局 zustand store 持有，避免离开设置页时下载进度丢失
  const appUpdate = useAppUpdate((s) => s.state);
  const checkUpdateApp = useAppUpdate((s) => s.check);
  const installUpdateApp = useAppUpdate((s) => s.install);
  // mihomo 内核更新同样接全局 store(逻辑跟应用自更新对齐:跨页面切换无缝继续)
  const checking = useMihomoUpdate((s) => s.checking);
  const installing = useMihomoUpdate((s) => s.installing);
  const updateInfo = useMihomoUpdate((s) => s.info);
  const installResult = useMihomoUpdate((s) => s.installResult);
  const checkUpdate = useMihomoUpdate((s) => s.check);
  const installUpdate = useMihomoUpdate((s) => s.install);
  const resetAppUpdate = useAppUpdate((s) => s.reset);
  const resetMihomoUpdate = useMihomoUpdate((s) => s.reset);
  // 离开设置页时清掉两个更新区的展示性终态(none/error/已是最新),否则下次
  // 打开会先闪一下旧的"已是最新版本",等真去查时才跳到"有新版本",误导用户。
  // store 里的 reset 自己做了进行中保护:checking/downloading/installing/available
  // 一律保留,让后台下载继续推进，这正是当初把状态从 useState 搬到 zustand 的目的。
  useEffect(() => {
    return () => {
      resetAppUpdate();
      resetMihomoUpdate();
    };
  }, [resetAppUpdate, resetMihomoUpdate]);
  // 二次确认对话框开关:第一次点"清理应用数据"只展开警告，第二次点确认才真正执行
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

  // 用户在二次确认对话框点"确认清理"才会进入这里:
  // 1. 调 clear_app_data 让 Rust 停后端 + 删 engine 目录
  // 2. Rust 端清理完成后会延迟 300ms 自行触发 app.restart() 重启进程,
  //    前端这里只需保持"清理中"状态等被打断即可，不需要也不能调 window.close()
  //    (后者会被 capabilities 默认 ACL 拒掉)
  // 3. 失败把 errors 显示出来，不重启应用，方便排查
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
              本地 <code className="text-xs">v{appUpdate.current}</code>
              {/* none 分支的 latest 来自后端 /checkappupdate(打 GitHub releases),
                  available/downloading 来自 plugin-updater。GitHub 不可达时 none.latest
                  为空串，此时只显示本地版本，不渲染"· 最新"以免出现"最新 v"的尾巴。 */}
              {((appUpdate.kind === "available" || appUpdate.kind === "downloading") ||
                (appUpdate.kind === "none" && appUpdate.latest)) && (
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
