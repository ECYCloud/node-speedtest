import { Link2, FileUp, Loader2, ClipboardPaste, Tag } from "lucide-react";
import { useTest } from "../../store/test";
import { Button, Card, Input, SectionTitle } from "../../components/ui";

export default function Importer() {
  const { url, importTab: tab, setUrl, setImportTab: setTab, loadSubscription, loadFileByPath, loadingConfigs, configs, error, group, setGroup } =
    useTest();

  async function onLoadSub() {
    const u = url.trim();
    if (!u) return;
    await loadSubscription(u);
  }

  // 选本地配置文件:仅用 Tauri 原生对话框选路径，读取 + 上传 + 解析全部交给 Rust
  // 端 import_config_file 完成(收敛链路，任何失败都会通过 store.error 显示出来)。
  // 不设 filters —— 有的机场配置是无后缀的纯文件名(如 "ECY Cloud")，写死后缀会
  // 把这类文件在对话框里过滤掉导致选不中;放开后任意文件都可选，格式由后端识别。
  async function onPickFile() {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const picked = await open({
      multiple: false,
      directory: false,
      title: "选择本地配置文件",
    });
    if (!picked || typeof picked !== "string") return; // 用户取消
    await loadFileByPath(picked);
  }

  // 从系统剪贴板粘贴订阅链接 — Webview 在 release 模式下默认禁用 navigator.clipboard,
  // 这里捕获异常优雅降级，用户仍可手动 Ctrl+V 到输入框。
  async function onPasteUrl() {
    try {
      const text = await navigator.clipboard.readText();
      if (text) setUrl(text.trim());
    } catch {
      /* 没权限或非安全上下文，忽略 */
    }
  }

  return (
    <Card className="p-5">
      <SectionTitle desc="支持订阅链接与本地配置文件">
        导入节点
      </SectionTitle>

      {/* 分组名:作为 PNG 标题 / 历史文件名前缀，留空则用第一个节点的协议默认 group */}
      <div className="relative mb-3">
        <Tag
          size={16}
          className="absolute left-4 top-1/2 -translate-y-1/2 text-fg-muted pointer-events-none"
        />
        <Input
          placeholder="分组名(可选，例:机场A)"
          value={group}
          onChange={(e) => setGroup(e.target.value)}
          disabled={loadingConfigs}
          className="pl-10"
        />
      </div>

      <div className="inline-flex items-center gap-1 p-1 rounded-full bg-border/40 mb-4 text-sm">
        {(["sub", "file"] as const).map((k) => (
          <button
            key={k}
            onClick={() => setTab(k)}
            className={
              "px-4 h-7 rounded-full transition " +
              (tab === k
                ? "bg-bg-elev text-fg shadow-sm"
                : "text-fg-muted hover:text-fg")
            }
          >
            {k === "sub" ? "订阅链接" : "本地文件"}
          </button>
        ))}
      </div>

      {tab === "sub" ? (
        <div className="flex flex-col gap-3">
          {/* 输入框占满整行，粘贴按钮内嵌右侧 */}
          <div className="relative">
            <Link2
              size={16}
              className="absolute left-4 top-1/2 -translate-y-1/2 text-fg-muted pointer-events-none"
            />
            <Input
              placeholder="粘贴订阅链接或单个节点链接"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && onLoadSub()}
              disabled={loadingConfigs}
              className="pl-10 pr-11"
            />
            <button
              type="button"
              disabled={loadingConfigs}
              onClick={onPasteUrl}
              title="从剪贴板粘贴"
              className="absolute right-2 top-1/2 -translate-y-1/2 inline-flex items-center justify-center w-7 h-7 rounded-full text-fg-muted hover:bg-border/50 disabled:opacity-40 transition"
            >
              <ClipboardPaste size={14} />
            </button>
          </div>
          {/* 读取按钮:与测试按钮一致 —— 左侧提示文字，右侧按钮 shrink-0 */}
          <div className="flex items-center justify-between gap-3 flex-wrap">
            <div className="text-xs text-fg-muted min-w-0 flex-1">
              推荐导入 Clash（mihomo/meta）订阅链接
            </div>
            <Button
              variant="primary"
              disabled={loadingConfigs || !url.trim()}
              onClick={onLoadSub}
              className="shrink-0 min-w-[6.5rem] justify-center"
            >
              {loadingConfigs ? (
                <>
                  <Loader2 size={14} className="animate-spin" />
                  读取中
                </>
              ) : (
                "读取"
              )}
            </Button>
          </div>
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          <div className="flex items-center gap-2">
            <Button
              variant="secondary"
              disabled={loadingConfigs}
              onClick={onPickFile}
            >
              <FileUp size={14} />
              选择文件
            </Button>
            <span className="text-xs text-fg-muted">
              支持 .json / .yaml / .conf / .txt 等
            </span>
          </div>
          <div className="text-xs text-fg-muted">
            推荐导入 Clash（mihomo/meta）配置文件
          </div>
        </div>
      )}

      {error && (
        <div className="mt-3 text-xs text-danger">{error}</div>
      )}
      {!error && configs.length > 0 && (
        <div className="mt-3 text-xs text-fg-muted">
          已识别 {configs.length} 个节点
        </div>
      )}
    </Card>
  );
}
