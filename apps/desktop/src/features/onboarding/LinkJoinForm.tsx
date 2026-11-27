import { type ClipboardEvent, type KeyboardEvent, useEffect, useRef, useState } from "react";
import { api, type CheckAfterRestart, type Session } from "../../lib/api";
import { errorMessage } from "../../lib/toast";
import { useFocusOnMount } from "../../lib/useFocusOnMount";
import { Button, CheckCode, Field, Spinner, StatusLine, spokenCheckCode, TextArea } from "../../ui";

/** How often a link picked up after a restart asks again for its number. */
const RECHECK_MS = 2_000;

type Step = { kind: "paste" } | { kind: "asking" } | { kind: "check"; check: CheckAfterRestart };

/**
 * Linking this device to you on another device: paste the code it shows
 * (or type it and press Enter), tap there the number this one shows, and
 * this device opens the workspace for reading at once. With `resumed`, a
 * link a restart interrupted goes on where it was; `ended` says why the
 * last one ended.
 */
export function LinkJoinForm({
  onReady,
  resumed,
  ended,
}: {
  onReady: (session: Session) => void;
  resumed?: CheckAfterRestart;
  ended?: string;
}) {
  const [code, setCode] = useState("");
  const [step, setStep] = useState<Step>(resumed ? { kind: "check", check: resumed } : { kind: "paste" });
  const [problem, setProblem] = useState<string | null>(ended ?? null);
  const [announcement, setAnnouncement] = useState("");
  const isStopping = useRef(false);
  const isWaiting = useRef(false);
  const field = useRef<HTMLTextAreaElement>(null);
  useFocusOnMount(field);

  const showCheck = (check: CheckAfterRestart) => {
    setStep({ kind: "check", check });
    if (check.kind === "code") setAnnouncement(`On your other device, tap ${spokenCheckCode(check.code)}`);
  };

  const waitForHandover = async () => {
    if (isWaiting.current) return;
    isWaiting.current = true;
    try {
      onReady(await api.waitForDeviceLinkHandover());
    } catch (error) {
      if (!isStopping.current) setProblem(errorMessage(error));
      isStopping.current = false;
      setStep({ kind: "paste" });
    } finally {
      isWaiting.current = false;
    }
  };

  // A link picked up after a restart asks again until it has its number or
  // is confirmed, then waits for the hand-over.
  const resumedCheck = step.kind === "check" && resumed ? step.check.kind : null;
  useEffect(() => {
    if (resumedCheck === null) return;
    if (resumedCheck === "code" || resumedCheck === "confirmed") {
      void waitForHandover();
      return;
    }
    const recheck = window.setTimeout(async () => {
      try {
        showCheck(await api.recheckDeviceLink());
      } catch (error) {
        setProblem(errorMessage(error));
        setStep({ kind: "paste" });
      }
    }, RECHECK_MS);
    return () => window.clearTimeout(recheck);
  });

  const tryCode = async (text: string) => {
    setCode(text);
    setProblem(null);
    if (text.trim().length === 0) return;
    try {
      await api.readLinkCode(text);
    } catch (error) {
      setProblem(errorMessage(error));
      return;
    }
    setStep({ kind: "asking" });
    try {
      const check = await api.beginDeviceLink(text);
      showCheck(check.code ? { kind: "code", code: check.code } : { kind: "confirmed" });
    } catch (error) {
      setProblem(errorMessage(error));
      setStep({ kind: "paste" });
      return;
    }
    await waitForHandover();
  };

  const onPaste = (event: ClipboardEvent<HTMLTextAreaElement>) => {
    event.preventDefault();
    void tryCode(event.clipboardData.getData("text/plain"));
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key !== "Enter" || event.shiftKey) return;
    event.preventDefault();
    void tryCode(code);
  };

  const pasteFromClipboard = async () => {
    try {
      await tryCode(await api.clipboardText());
    } catch (error) {
      setProblem(errorMessage(error));
    }
  };

  const stop = () => {
    isStopping.current = true;
    api.stopDeviceLink().then(
      () => setStep({ kind: "paste" }),
      (error: unknown) => setProblem(errorMessage(error)),
    );
  };

  return (
    <>
      <p role="status" className="sr-only">
        {announcement}
      </p>
      {step.kind === "asking" && (
        <div className="flex flex-col items-center gap-3 py-6">
          <Spinner />
          <StatusLine>Asking your other device…</StatusLine>
        </div>
      )}
      {step.kind === "check" && <CheckStep check={step.check} onStop={stop} />}
      {step.kind === "paste" && (
        <div className="flex flex-col gap-3">
          <Field
            label="Link code"
            hint="On your other device, open Settings › Devices › Link a device. Paste the code, or type it and press Enter."
            error={problem ?? undefined}
          >
            {(id) => (
              <TextArea
                id={id}
                ref={field}
                value={code}
                onChange={(event) => {
                  setCode(event.target.value);
                  setProblem(null);
                }}
                onPaste={onPaste}
                onKeyDown={onKeyDown}
                placeholder="shyl1_…"
                rows={4}
                className="font-mono text-xs"
              />
            )}
          </Field>
          <div>
            <Button variant="secondary" onClick={() => void pasteFromClipboard()}>
              Paste
            </Button>
          </div>
        </div>
      )}
    </>
  );
}

/** What this device shows while the person's other device decides. */
function CheckStep({ check, onStop }: { check: CheckAfterRestart; onStop: () => void }) {
  return (
    <div className="flex flex-col items-center gap-4 py-4 text-center">
      {check.kind === "code" && (
        <>
          <p className="text-sm">On your other device, tap this number</p>
          <CheckCode code={check.code} />
          <StatusLine>There's nothing to tap here.</StatusLine>
        </>
      )}
      {check.kind === "confirmed" && <StatusLine>Confirmed on your other device. Opening the workspace…</StatusLine>}
      {check.kind === "not_yet" && (
        <StatusLine>Waiting for your other device to open this request. Keep its Link a device window open.</StatusLine>
      )}
      {check.kind === "unreachable" && <StatusLine>Can't reach your workspace right now. Still trying…</StatusLine>}
      <Button variant="ghost" onClick={onStop}>
        {check.kind === "code" || check.kind === "confirmed" ? "Cancel" : "Start again"}
      </Button>
    </div>
  );
}
