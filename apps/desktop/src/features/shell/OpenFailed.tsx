import { Button, Logo } from "../../ui";

/**
 * Shown when the workspaces on this device exist but wouldn't open, so it
 * never looks as if they were gone: the reason, a way to try again, and,
 * for a failure that stays, a way to set up a different workspace.
 */
export function OpenFailed({
  reason,
  onRetry,
  onSetUpAnother,
}: {
  reason: string;
  onRetry: () => void;
  onSetUpAnother: () => void;
}) {
  return (
    <main className="grid h-full place-items-center p-6">
      <div className="flex max-w-card flex-col items-center gap-4 text-center">
        <Logo size={44} />
        <h1 className="text-lg font-semibold">shyspace couldn't open your workspace</h1>
        <p className="text-sm text-muted">{reason}</p>
        <Button variant="primary" onClick={onRetry}>
          Try again
        </Button>
        <div className="flex flex-col items-center gap-1">
          <Button variant="ghost" onClick={onSetUpAnother}>
            Set up another workspace
          </Button>
          <p className="text-xs text-muted">
            Joining or restoring this same workspace won't work while it's still on this device.
          </p>
        </div>
      </div>
    </main>
  );
}
