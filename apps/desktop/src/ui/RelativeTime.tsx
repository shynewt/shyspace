import { formatFullDate, formatWhen } from "../lib/people";
import { Tooltip } from "./Tooltip";

/** A past moment, as short as stays clear ("Yesterday 4:41 PM"); the full date shows on hover. */
export function RelativeTime({ ms, className }: { ms: number; className?: string }) {
  return (
    <Tooltip label={formatFullDate(ms)}>
      <time dateTime={new Date(ms).toISOString()} className={className}>
        {formatWhen(ms, new Date())}
      </time>
    </Tooltip>
  );
}
