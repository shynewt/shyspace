import { Button, Logo } from "../../ui";

/**
 * Shown when the workspaces on this device exist but wouldn't open, so it
 * never looks as if they were gone: the reason, and a way to try again.
 */
export function OpenFailed({ reason, onRetry }: { reason: string; onRetry: () => void }) {
  return (
    <main className="grid h-full place-items-center p-6">
      <div className="flex max-w-card flex-col items-center gap-4 text-center">
        <Logo size={44} />
        <h1 className="text-lg font-semibold">shyspace couldn't open your workspace</h1>
        <p className="text-sm text-muted">{reason}</p>
        <Button variant="primary" onClick={onRetry}>
          Try again
        </Button>
      </div>
    </main>
  );
}
