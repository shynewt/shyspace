import { MotionConfig } from "motion/react";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { springs, TooltipProvider } from "./ui";
import "./styles.css";

const root = document.getElementById("root");
if (!root) throw new Error("index.html has no #root element");

createRoot(root).render(
  <StrictMode>
    <MotionConfig reducedMotion="user" transition={springs.snappy}>
      <TooltipProvider>
        <App />
      </TooltipProvider>
    </MotionConfig>
  </StrictMode>,
);
