import { useState } from "react";
import { reportError } from "../lib/toast";
import { Button } from "./Button";
import { Dialog } from "./Dialog";

/**
 * Asks before something that can't be undone. Stays open, with the error
 * shown, if the action fails.
 */
export function ConfirmDialog({
  open,
  onOpenChange,
  title,
  description,
  confirmLabel,
  onConfirm,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description: string;
  confirmLabel: string;
  onConfirm: () => Promise<void>;
}) {
  const [isBusy, setBusy] = useState(false);
  const confirm = async () => {
    setBusy(true);
    try {
      await onConfirm();
      onOpenChange(false);
    } catch (error) {
      reportError(error);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title={title} description={description} width={400}>
      <div className="flex justify-end gap-2 p-5">
        <Button variant="ghost" onClick={() => onOpenChange(false)}>
          Cancel
        </Button>
        <Button variant="danger" isLoading={isBusy} onClick={confirm}>
          {confirmLabel}
        </Button>
      </div>
    </Dialog>
  );
}
