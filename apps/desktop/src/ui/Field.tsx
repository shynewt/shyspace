import { type ReactNode, useId } from "react";

/**
 * A labeled form control, with optional help text. `children` receives the id
 * to put on the control, so the label is properly linked to it.
 */
export function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: ReactNode;
  children: (id: string) => ReactNode;
}) {
  const id = useId();
  return (
    <div className="flex min-w-0 flex-col gap-1.5">
      <label htmlFor={id} className="text-[12px] font-medium text-muted">
        {label}
      </label>
      {children(id)}
      {hint && <span className="text-[11.5px] leading-relaxed text-faint">{hint}</span>}
    </div>
  );
}
