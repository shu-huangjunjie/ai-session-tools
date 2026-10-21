import { Component, ErrorInfo, ReactNode } from 'react';

interface Props {
  children: ReactNode;
}

interface State {
  hasError: boolean;
  error: Error | null;
  errorInfo: ErrorInfo | null;
}

export class ErrorBoundary extends Component<Props, State> {
  public state: State = {
    hasError: false,
    error: null,
    errorInfo: null,
  };

  public static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error, errorInfo: null };
  }

  public componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    console.error('Uncaught error:', error, errorInfo);
    this.setState({ errorInfo });
  }

  public render() {
    if (this.state.hasError) {
      return (
        <div className="w-screen h-screen flex flex-col items-center justify-center bg-[#06101c] text-[#dceaf6] p-8">
          <div className="max-w-xl w-full bg-[#0a192f] border border-red-500/40 rounded-xl p-6 shadow-2xl">
            <div className="flex items-center space-x-3 mb-4">
              <div className="w-4 h-4 rounded-full bg-red-500 animate-pulse" />
              <h2 className="text-xl font-bold text-red-400">页面渲染异常 (Error Boundary)</h2>
            </div>
            <p className="text-sm text-gray-300 mb-4 leading-relaxed">
              客户端捕获到未处理的前端异常，已阻止页面陷入完全空白。您可以查看错误信息或刷新重试。
            </p>
            <div className="bg-[#030811] p-3 rounded-lg border border-gray-800 text-xs text-red-300 font-mono overflow-auto max-h-48 mb-4 whitespace-pre-wrap">
              {this.state.error?.toString()}
              {this.state.errorInfo?.componentStack}
            </div>
            <div className="flex justify-end space-x-3">
              <button
                onClick={() => window.location.reload()}
                className="px-4 py-2 bg-blue-600 hover:bg-blue-500 text-white rounded-lg text-sm font-medium transition"
              >
                刷新页面
              </button>
            </div>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}
