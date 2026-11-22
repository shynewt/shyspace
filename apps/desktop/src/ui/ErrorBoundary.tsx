import { Component, type ErrorInfo, type ReactNode } from "react";

type Props = { fallback: ReactNode; children: ReactNode };

/**
 * Shows `fallback` instead of `children` once drawing them throws, so one
 * broken part can't blank everything around it. Give it a `key` that changes
 * with the content to try again.
 */
export class ErrorBoundary extends Component<Props, { hasFailed: boolean }> {
  state = { hasFailed: false };

  static getDerivedStateFromError(): { hasFailed: boolean } {
    return { hasFailed: true };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error("part of the screen failed to draw", error, info.componentStack);
  }

  render(): ReactNode {
    return this.state.hasFailed ? this.props.fallback : this.props.children;
  }
}
