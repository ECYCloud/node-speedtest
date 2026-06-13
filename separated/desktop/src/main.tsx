import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ErrorBoundary } from "./components/ErrorBoundary";
import "./index.css";

// 禁用 webview 默认右键菜单(刷新/查看源码等开发者菜单对最终用户无用)
document.addEventListener("contextmenu", (e) => e.preventDefault());

// Tauri 窗口启动时设了 visible:false(避免白屏闪烁),React 第一帧画完后再让窗口显示。
// 任何分支异常都把错误打到 console,Linux/macOS 上即使第一次 invoke 失败也重试,
// 让窗口尽可能能显示;主进程那边还有 5 秒兜底 timer 强制 show,这里只是更快路径。
async function showMainWindow() {
  for (let i = 0; i < 5; i++) {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("show_main_window");
      return;
    } catch (e) {
      console.error("[showMainWindow] attempt", i, e);
      await new Promise((r) => setTimeout(r, 300));
    }
  }
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </React.StrictMode>,
);

// 等两帧再 show:第一帧 React 提交 DOM，第二帧浏览器完成布局/绘制，
// 窗口出现的瞬间用户看到的就是已渲染好的主界面，不会再有任何闪烁。
requestAnimationFrame(() =>
  requestAnimationFrame(() => {
    showMainWindow();
  }),
);
