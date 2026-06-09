import { Button, Card, SectionTitle } from "../components/ui";
import { RefreshCw, Monitor, Download, ExternalLink, CheckCircle2, AlertCircle, PackageCheck } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
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

export default function SettingsPage() {
  const [restarting, setRestarting] = useState(false);
  const [checking, setChecking] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [installing, setInstalling] = useState(false);
  const [installResult, setInstallResult] = useState<UpdateResult | null>(null);

  async function restart() {
    setRestarting(true);
    try {
      await invoke("restart_backend");
    } finally {
      setRestarting(false);
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
