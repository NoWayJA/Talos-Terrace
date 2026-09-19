import { Component, type ErrorInfo, type ReactNode } from 'react';

interface ErrorBoundaryProps {
  children: ReactNode;
}

interface ErrorBoundaryState {
  error: Error | null;
}

/** Catches render errors below it and shows a recoverable screen instead of a blank window. */
export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  override state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  override componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error('Unhandled render error', error, info.componentStack);
  }

  private readonly reload = (): void => {
    window.location.reload();
  };

  override render(): ReactNode {
    if (this.state.error) {
      return (
        <div className="error-screen" role="alert">
          <h1 className="error-screen__title">Something went wrong</h1>
          <pre className="error-screen__message">{this.state.error.message}</pre>
          <button type="button" className="btn btn--primary" onClick={this.reload}>
            Reload
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}
