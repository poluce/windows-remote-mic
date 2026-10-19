import { Component, type ErrorInfo, type ReactNode } from "react";

type Props = { children: ReactNode };
type State = { error: Error | null };

/**
 * 界面异常兜底。
 *
 * 前端渲染崩溃时如果什么都不做，窗口会整片空白；而后端的按键/语音线程其实
 * 还在正常工作。这里把错误拦在 React 树内，给出可恢复的提示面板——
 * 用户至少知道「设备没坏，只是界面出问题了」，点一下就能重载。
 */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("[ui] 界面渲染异常", error, info);
  }

  private reload = () => {
    window.location.reload();
  };

  render() {
    const { error } = this.state;
    if (!error) {
      return this.props.children;
    }
    return (
      <div className="ui-crash">
        <div className="ui-crash-card">
          <h2>界面出现异常</h2>
          <p className="ui-crash-hint">
            已捕获错误，遥控器连接与语音桥不受影响。
            可以点下方按钮重新加载界面，或重启应用。
          </p>
          <pre className="ui-crash-detail">
            {String(error.message || error)}
          </pre>
          <button className="btn primary" onClick={this.reload}>
            重新加载界面
          </button>
        </div>
      </div>
    );
  }
}
