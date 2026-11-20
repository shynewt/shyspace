import { type ReactNode, useState } from "react";
import { reportError } from "../lib/toast";
import { Button } from "./Button";
import { Dialog, DialogActions } from "./Dialog";

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
  children,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description: string;
  confirmLabel: string;
  onConfirm: () => Promise<void>;
  /** What the action applies to, shown above the buttons. */
  children?: ReactNode;
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
      {children && <div>{children}</div>}
      <DialogActions>
        <Button variant="ghost" onClick={() => onOpenChange(false)}>
          Cancel
        </Button>
        <Button variant="danger" isLoading={isBusy} onClick={confirm}>
          {confirmLabel}
        </Button>
      </DialogActions>
    </Dialog>
  );
}
