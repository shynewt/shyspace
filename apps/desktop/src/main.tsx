import { MotionConfig } from "motion/react";
import { type ReactNode, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { useReducedMotionConfig } from "./lib/preferences";
import { springs, TooltipProvider } from "./ui";
import "./styles.css";

const root = document.getElementById("root");
if (!root) throw new Error("index.html has no #root element");

createRoot(root).render(
  <StrictMode>
    <ChosenMotion>
      <TooltipProvider>
        <App />
      </TooltipProvider>
    </ChosenMotion>
  </StrictMode>,
);

/** Motion everywhere follows the Reduce motion setting (by default, the system's). */
function ChosenMotion({ children }: { children: ReactNode }) {
  return (
    <MotionConfig reducedMotion={useReducedMotionConfig()} transition={springs.snappy}>
      {children}
    </MotionConfig>
  );
}
