import { useEffect, useRef, useState } from "react";
import { api, type LinkCheck, type Session } from "../../lib/api";
import { errorMessage } from "../../lib/toast";
import { useFocusOnMount } from "../../lib/useFocusOnMount";
import { Button, CheckCode, Field, Spinner, StatusLine, TextArea } from "../../ui";

type Step = { kind: "paste" } | { kind: "asking" } | { kind: "check"; code: string | null };

/**
 * Linking this device to you on another device: paste or type the code it
 * shows, tap there the number this one shows, and this device opens the
 * workspace for reading at once. With `resumed`, a link a restart
 * interrupted goes on where it was.
 */
export function LinkJoinForm({ onReady, resumed }: { onReady: (session: Session) => void; resumed?: LinkCheck }) {
  const [code, setCode] = useState("");
  const [step, setStep] = useState<Step>(resumed ? { kind: "check", code: resumed.code } : { kind: "paste" });
  const [problem, setProblem] = useState<string | null>(null);
  const isStopping = useRef(false);
  const field = useRef<HTMLTextAreaElement>(null);
  useFocusOnMount(field);

  const waitForHandover = async () => {
    try {
      onReady(await api.waitForDeviceLinkHandover());
    } catch (error) {
      if (!isStopping.current) setProblem(errorMessage(error));
      isStopping.current = false;
      setStep({ kind: "paste" });
    }
  };

  const hasResumed = useRef(false);
  useEffect(() => {
    if (!resumed || hasResumed.current) return;
    hasResumed.current = true;
    void waitForHandover();
  });

  const take = async (text: string) => {
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
      setStep({ kind: "check", code: check.code });
    } catch (error) {
      setProblem(errorMessage(error));
      setStep({ kind: "paste" });
      return;
    }
    await waitForHandover();
  };

  const stop = () => {
    isStopping.current = true;
    void api.stopDeviceLink().catch((error: unknown) => setProblem(errorMessage(error)));
  };

  if (step.kind === "asking") {
    return (
      <div className="flex flex-col items-center gap-3 py-6">
        <Spinner />
        <StatusLine>Asking your other device…</StatusLine>
      </div>
    );
  }
  if (step.kind === "check") {
    return (
      <div className="flex flex-col items-center gap-4 py-4 text-center">
        {step.code ? (
          <>
            <p className="text-sm">On your other device, tap this number</p>
            <CheckCode code={step.code} />
          </>
        ) : (
          <p className="text-sm">Confirmed on your other device. Opening the workspace…</p>
        )}
        <StatusLine>There's nothing to tap here.</StatusLine>
        <Button variant="ghost" onClick={stop}>
          Cancel
        </Button>
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-3">
      <Field
        label="Link code"
        hint="On your other device, open Settings › Devices › Link a device."
        error={problem ?? undefined}
      >
        {(id) => (
          <TextArea
            id={id}
            ref={field}
            value={code}
            onChange={(event) => void take(event.target.value)}
            placeholder="shyl1_…"
            rows={4}
            className="font-mono text-xs"
          />
        )}
      </Field>
    </div>
  );
}
