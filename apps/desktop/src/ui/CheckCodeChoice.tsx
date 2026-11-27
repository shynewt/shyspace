import { Button } from "./Button";
import { CheckCode } from "./CheckCode";

/**
 * Three check codes to tap the one another device shows, and "None of
 * these". Nothing is preselected or focused: the person has to look.
 */
export function CheckCodeChoice({
  choices,
  label,
  isBusy = false,
  onPick,
  onNone,
}: {
  choices: readonly string[];
  /** Names the group for screen readers, like the heading above it. */
  label: string;
  isBusy?: boolean;
  onPick: (code: string) => void;
  onNone: () => void;
}) {
  return (
    <div className="flex flex-col items-center gap-3">
      <fieldset className="m-0 flex w-full min-w-0 flex-col gap-2 border-0 p-0">
        <legend className="sr-only">{label}</legend>
        {choices.map((code) => (
          <button
            key={code}
            type="button"
            disabled={isBusy}
            onClick={() => onPick(code)}
            className="rounded-xl border border-line bg-surface px-4 py-3 hover:border-surface-3 hover:bg-surface-2 disabled:opacity-60"
          >
            <CheckCode code={code} />
          </button>
        ))}
      </fieldset>
      <Button variant="ghost" disabled={isBusy} onClick={onNone}>
        None of these
      </Button>
    </div>
  );
}
