import { motion } from "motion/react";
import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../../lib/api";
import { errorMessage, reportError } from "../../lib/toast";
import { Button, CheckCodeChoice, CopyField, Dialog, DialogActions, fade, QrCode, Spinner, StatusLine } from "../../ui";

/** How often the dialog asks whether the new device has asked, while showing the code. */
const POLL_MS = 2_000;
/** A code works for 10 minutes: one about to run out is replaced while the dialog is open. */
const RENEW_MS = 9 * 60 * 1000;
/** How long "Linked" stays before the dialog closes itself. */
const DONE_MS = 4_000;

type Step =
  | { kind: "loading" }
  | { kind: "refused"; message: string }
  | { kind: "code"; code: string }
  | { kind: "pick"; choices: [string, string, string] }
  | { kind: "stopped"; message: string }
  | { kind: "done"; isFinishing: boolean };

const LINKED = "Linked. Your new device can read now, and can send once your workspace adds it.";
const FINISHING = "Linked, finishing… It goes on as soon as your workspace can be reached.";
const UNREACHABLE = "Can't reach your workspace right now. Still watching for your new device…";

/**
 * Links a new device of yours: shows a code for it, then the number it
 * shows among two decoys, to tap. A wrong pick or "None of these" ends the
 * link; closing the dialog stops it too.
 */
export function LinkDeviceDialog({ onClose }: { onClose: () => void }) {
  const [step, setStep] = useState<Step>({ kind: "loading" });
  const [isBusy, setBusy] = useState(false);
  const [isUnreachable, setUnreachable] = useState(false);
  const heading = useRef<HTMLHeadingElement>(null);
  const result = useRef<HTMLParagraphElement>(null);

  const makeCode = useCallback(async () => {
    setStep({ kind: "loading" });
    try {
      setStep({ kind: "code", code: await api.createLinkCode() });
    } catch (error) {
      setStep({ kind: "refused", message: errorMessage(error) });
    }
  }, []);

  useEffect(() => {
    void makeCode();
  }, [makeCode]);

  // While the code shows: watch for the new device, and renew the code before it runs out.
  useEffect(() => {
    if (step.kind !== "code") return;
    const poll = window.setInterval(async () => {
      try {
        const shown = await api.pendingLink();
        setUnreachable(false);
        if (shown) setStep({ kind: "pick", choices: shown.choices });
      } catch {
        // Polled every 2 s: a failure is a quiet line until the next one works.
        setUnreachable(true);
      }
    }, POLL_MS);
    const renew = window.setTimeout(() => void makeCode(), RENEW_MS);
    return () => {
      window.clearInterval(poll);
      window.clearTimeout(renew);
    };
  }, [step.kind, makeCode]);

  // Focus goes to the heading, never to a number: the person has to look.
  // A result takes focus, so it is read out.
  useEffect(() => {
    if (step.kind === "pick") heading.current?.focus();
    if (step.kind === "done" || step.kind === "stopped") result.current?.focus();
  }, [step.kind]);

  useEffect(() => {
    if (step.kind !== "done") return;
    const close = window.setTimeout(onClose, DONE_MS);
    return () => window.clearTimeout(close);
  }, [step.kind, onClose]);

  const pick = async (choice: string) => {
    setBusy(true);
    try {
      const confirmed = await api.confirmLink(choice);
      setStep({ kind: "done", isFinishing: confirmed.is_finishing });
    } catch (error) {
      setStep({ kind: "stopped", message: errorMessage(error) });
    } finally {
      setBusy(false);
    }
  };

  const noneOfThese = async () => {
    setBusy(true);
    try {
      await api.cancelLink();
    } catch (error) {
      reportError(error);
    } finally {
      setBusy(false);
      setStep({ kind: "stopped", message: "Stopped. Nothing was shared." });
    }
  };

  const close = () => {
    if (step.kind === "code" || step.kind === "pick") void api.cancelLink().catch(reportError);
    onClose();
  };

  return (
    <Dialog open onOpenChange={(open) => !open && close()} title="Link a device" hideTitle width={420}>
      <motion.div key={step.kind} {...fade} className="flex flex-col items-center gap-4 text-center">
        {step.kind === "loading" && <Spinner />}
        {step.kind === "refused" && <p className="text-sm">{step.message}</p>}
        {step.kind === "code" && (
          <>
            <h2 className="text-base font-semibold">Link a device</h2>
            <p className="text-sm text-muted">
              The new device will see the workspace's history and channels, not private channels or DMs yet.
            </p>
            <QrCode value={step.code} label="The link code, to scan on your new device" />
            <div className="w-full text-left">
              <CopyField value={step.code} label="Copy the link code" />
            </div>
            <StatusLine>
              {isUnreachable ? UNREACHABLE : 'On your new device, choose "I already use shyspace".'}
            </StatusLine>
          </>
        )}
        {step.kind === "pick" && (
          <>
            <h2 ref={heading} tabIndex={-1} className="text-base font-semibold outline-none">
              Tap the number your new device shows
            </h2>
            <CheckCodeChoice
              choices={step.choices}
              label="Tap the number your new device shows"
              isBusy={isBusy}
              onPick={pick}
              onNone={noneOfThese}
            />
            <p className="text-xs text-muted">Only continue if you're linking your own device right now.</p>
          </>
        )}
        {step.kind === "stopped" && (
          <>
            <p ref={result} tabIndex={-1} className="text-sm outline-none">
              {step.message}
            </p>
            <Button variant="primary" onClick={() => void makeCode()}>
              New code
            </Button>
          </>
        )}
        {step.kind === "done" && (
          <p ref={result} tabIndex={-1} className="text-sm outline-none">
            {step.isFinishing ? FINISHING : LINKED}
          </p>
        )}
      </motion.div>
      <DialogActions>
        <Button variant="ghost" onClick={close}>
          {step.kind === "done" ? "Done" : "Close"}
        </Button>
      </DialogActions>
    </Dialog>
  );
}
