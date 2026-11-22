import { TriangleAlert } from "lucide-react";
import { MotionConfig } from "motion/react";
import { type ReactNode, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { forgetLastView } from "./lib/lastView";
import { useReducedMotionConfig } from "./lib/preferences";
import { Button, EmptyState, ErrorBoundary, TooltipProvider, tweens } from "./ui";
import "./styles.css";

const root = document.getElementById("root");
if (!root) throw new Error("index.html has no #root element");

createRoot(root).render(
  <StrictMode>
    <ErrorBoundary fallback={<ScreenFailed />}>
      <ChosenMotion>
        <TooltipProvider>
          <App />
        </TooltipProvider>
      </ChosenMotion>
    </ErrorBoundary>
  </StrictMode>,
);

/** Motion everywhere follows the Reduce motion setting (by default, the system's). */
function ChosenMotion({ children }: { children: ReactNode }) {
  return (
    <MotionConfig reducedMotion={useReducedMotionConfig()} transition={tweens.base}>
      {children}
    </MotionConfig>
  );
}

/** The app reopens where it was, so going back there could fail again; this starts from the first channel. */
function ScreenFailed() {
  const startOver = () => {
    forgetLastView();
    window.location.reload();
  };
  return (
    <div role="alert" className="h-screen bg-bg text-ink">
      <EmptyState
        icon={<TriangleAlert size={22} aria-hidden />}
        title="Something went wrong showing this screen"
        action={<Button onClick={startOver}>Go to the first channel</Button>}
      >
        Nothing was lost. Starting from the first channel usually gets you going again.
      </EmptyState>
    </div>
  );
}
