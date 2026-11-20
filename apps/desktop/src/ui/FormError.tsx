import { CircleAlert } from "lucide-react";

/** Why a form's last save failed, shown under it and announced to screen readers. */
export function FormError({ message }: { message: string | null }) {
  if (!message) return null;
  return (
    <p role="alert" className="selectable flex gap-1.5 text-[12px] leading-relaxed text-danger-text">
      <CircleAlert size={14} className="mt-0.5 shrink-0" />
      {message}
    </p>
  );
}
