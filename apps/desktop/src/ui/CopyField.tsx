import { CopyButton } from "./CopyButton";

/** Read-only text with a copy button, for codes and commands. */
export function CopyField({ value, label = "Copy" }: { value: string; label?: string }) {
  return (
    <div className="relative">
      <pre className="selectable max-h-28 overflow-auto rounded-lg border border-line bg-surface p-2 pr-10 font-mono text-2xs leading-relaxed break-all whitespace-pre-wrap text-muted">
        {value}
      </pre>
      <div className="absolute top-1.5 right-1.5">
        <CopyButton value={value} label={label} className="bg-surface-2" />
      </div>
    </div>
  );
}
