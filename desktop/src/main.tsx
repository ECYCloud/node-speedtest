import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ErrorBoundary } from "./components/ErrorBoundary";
import "./index.css";

// 禁用 webview 默认右键菜单(刷新/查看源码等开发者菜单对最终用户无用)
document.addEventListener("contextmenu", (e) => e.preventDefault());

// React 挂载完成后移除启动 splash(index.html 里那个紫色加载页)
function dismissSplash() {
  const el = document.getElementById("ss-splash");
  if (!el) return;
  el.classList.add("hide");
  setTimeout(() => el.remove(), 250);
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </React.StrictMode>,
);

// 等下一帧确保 React 实际渲染了一次再隐藏 splash,
// 否则 splash 消失瞬间空白会闪一下
requestAnimationFrame(() => requestAnimationFrame(dismissSplash));
