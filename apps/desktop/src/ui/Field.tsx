import { type ReactNode, useId } from "react";

/** Props that link a field's control to its hint and error. */
export type FieldDescription = {
  "aria-describedby": string | undefined;
  "aria-invalid": boolean;
};

/**
 * A labeled form control, with optional help text and an error that says how
 * to fix it. `children` receives the id to put on the control, so the label
 * is properly linked to it, and props that link the hint and error too.
 */
export function Field({
  label,
  hint,
  error,
  children,
}: {
  label: string;
  hint?: ReactNode;
  /** Shown under the control, in place of the hint, while set. */
  error?: string | null;
  children: (id: string, description: FieldDescription) => ReactNode;
}) {
  const id = useId();
  const hintId = `${id}-hint`;
  const errorId = `${id}-error`;
  const description: FieldDescription = {
    "aria-describedby": error ? errorId : hint ? hintId : undefined,
    "aria-invalid": Boolean(error),
  };
  return (
    <div className="flex min-w-0 flex-col gap-1.5">
      <label htmlFor={id} className="text-xs font-medium text-muted">
        {label}
      </label>
      {children(id, description)}
      {error ? (
        <span id={errorId} className="text-xs leading-relaxed text-danger-text">
          {error}
        </span>
      ) : (
        hint && (
          <span id={hintId} className="text-xs leading-relaxed text-faint">
            {hint}
          </span>
        )
      )}
    </div>
  );
}
