import { Component, ReactNode } from "react";
import { AlertTriangle, RefreshCw, Home } from "lucide-react";
import { useTest } from "../store/test";

interface State {
  err: Error | null;
}

/** 兜底错误边界:任何渲染期异常都不会让 webview 整页空白，
    而是显示友好错误信息 + 返回主页/重试，保证用户能自救而不必关软件。 */
export class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { err: null };

  static getDerivedStateFromError(err: Error): State {
    return { err };
  }

  componentDidCatch(err: Error, info: unknown) {
    // eslint-disable-next-line no-console
    console.error("[ErrorBoundary]", err, info);
  }

  /** 返回主页:清空 store 中可能导致渲染崩溃的脏数据，然后重置错误边界 */
  resetToHome = () => {
    useTest.setState({
      configs: [],
      selected: new Set(),
      results: [],
      current: null,
      error: null,
      filter: "",
      typeFilter: new Set(),
      status: "stopped",
      starting: false,
      targetCount: 0,
    });
    this.setState({ err: null });
  };

  render() {
    if (!this.state.err) return this.props.children;
    return (
      <div className="h-full w-full flex items-center justify-center bg-bg text-fg p-8">
        <div className="max-w-lg w-full rounded-2xl border border-border bg-bg-elev shadow-sm p-6">
          <div className="flex items-center gap-2 text-danger font-semibold mb-2">
            <AlertTriangle size={18} />
            页面渲染出错
          </div>
          <div className="text-sm text-fg-muted mb-1">
            软件遇到了一个未预料的错误，可以尝试以下操作:
          </div>
          <pre className="text-xs text-fg-muted bg-border/30 rounded-md p-2.5 my-3 break-all whitespace-pre-wrap">
            {this.state.err.message}
          </pre>
          <div className="flex items-center gap-2">
            <button
              onClick={this.resetToHome}
              className="inline-flex items-center gap-2 h-9 px-5 rounded-full bg-primary text-primary-fg text-sm font-medium shadow-sm shadow-primary/30 hover:brightness-110 active:scale-[0.98] transition"
            >
              <Home size={14} />
              返回主页
            </button>
            <button
              onClick={() => this.setState({ err: null })}
              className="inline-flex items-center gap-2 h-9 px-5 rounded-full border border-border bg-bg-elev text-sm hover:bg-border/40 active:scale-[0.98] transition"
            >
              <RefreshCw size={14} />
              重试当前页
            </button>
          </div>
        </div>
      </div>
    );
  }
}
