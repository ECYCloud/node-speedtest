import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ErrorBoundary } from "./components/ErrorBoundary";
import "./index.css";

// 禁用 webview 默认右键菜单(刷新/查看源码等开发者菜单对最终用户无用)
document.addEventListener("contextmenu", (e) => e.preventDefault());

// Tauri 窗口启动时设了 visible:false(避免白屏闪烁),React 第一帧画完后再让窗口显示。
// 浏览器环境(纯 web 调试)下 invoke 不存在，catch 静默忽略即可。
async function showMainWindow() {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("show_main_window");
  } catch {
    /* 非 Tauri 环境 */
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
