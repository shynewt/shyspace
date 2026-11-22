/**
 * A day's heading in a dated list ("Today", "Monday, April 27"): the label
 * between two hairlines. It has no side margin of its own, so it spans what
 * the list gives it and lines up with the rows below.
 */
export function DayDivider({ label }: { label: string }) {
  return (
    <div className="my-3 flex items-center gap-3">
      <div className="h-px flex-1 bg-line" />
      <span className="text-xs font-medium text-faint">{label}</span>
      <div className="h-px flex-1 bg-line" />
    </div>
  );
}
