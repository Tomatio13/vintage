/**
 * React error boundary.
 *
 * Without one, a render-time exception unmounts the entire component tree and
 * blanks the whole app. Wrapping a subtree (e.g. the Files panel) confines a
 * crash to that subtree so the terminal workspace keeps running.
 */

import { Component, type ErrorInfo, type ReactNode } from "react";

export interface ErrorBoundaryProps {
  children: ReactNode;
  /** Rendered in place of `children` after a caught error. */
  fallback?: (error: Error, onReset: () => void) => ReactNode;
}

interface ErrorBoundaryState {
  error: Error | null;
}

export function defaultErrorFallback(error: Error, onReset: () => void) {
  return (
    <div className="error-boundary-fallback" role="alert">
      <strong>Something went wrong here.</strong>
      {error.message && <p>{error.message}</p>}
      <button type="button" onClick={onReset}>
        Reload
      </button>
    </div>
  );
}

export class ErrorBoundary extends Component<
  ErrorBoundaryProps,
  ErrorBoundaryState
> {
  state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    // Keep the stack for debugging; the user-facing message is in state.
    console.error("ErrorBoundary caught:", error, info.componentStack);
  }

  private handleReset = () => {
    this.setState({ error: null });
  };

  render() {
    if (this.state.error) {
      return this.props.fallback
        ? this.props.fallback(this.state.error, this.handleReset)
        : defaultErrorFallback(this.state.error, this.handleReset);
    }
    return this.props.children;
  }
}
