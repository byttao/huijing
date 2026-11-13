import { Component, type ErrorInfo, type ReactNode } from "react";
import { Button, Result } from "antd";

interface State {
  error: Error | null;
}

/** 全局错误边界：界面渲染出错时显示原因与「重新加载」，而不是整页空白。 */
export class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("界面渲染出错", error, info.componentStack);
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="grid h-screen place-items-center bg-layout p-8">
        <Result
          status="error"
          title="界面出错了"
          subTitle={<span className="break-all font-mono text-sm">{this.state.error.message}</span>}
          extra={
            <Button type="primary" onClick={() => window.location.reload()}>
              重新加载
            </Button>
          }
        />
      </div>
    );
  }
}
