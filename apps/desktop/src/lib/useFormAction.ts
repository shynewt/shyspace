import { type FormEvent, useState } from "react";
import { errorMessage } from "./toast";

/**
 * Runs a form's save and tracks it: busy while it runs, and the error if it
 * fails. The form's fields are left alone, so a failed save loses nothing.
 */
export function useFormAction(action: () => Promise<void>) {
  const [isBusy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = async (event?: FormEvent) => {
    event?.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (caught) {
      console.error(caught);
      setError(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  };
  return { run, isBusy, error };
}
