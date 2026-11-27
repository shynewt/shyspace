import { useState } from "react";
import { Button, ConfirmDialog } from "../../ui";
import { useWorkspaces } from "../shell/Workspaces";

/** Stops this device's link, asking first: what it read is wiped. */
export function StopLinkingButton() {
  const { stopLink } = useWorkspaces();
  const [isConfirming, setConfirming] = useState(false);
  return (
    <>
      <Button size="sm" variant="ghost" onClick={() => setConfirming(true)}>
        Stop linking
      </Button>
      <ConfirmDialog
        open={isConfirming}
        onOpenChange={setConfirming}
        title="Stop linking this device?"
        description="Nothing it read stays on it. To use it, you'd link it again from your other device."
        confirmLabel="Stop linking"
        cancelLabel="Keep linking"
        onConfirm={stopLink}
      />
    </>
  );
}
