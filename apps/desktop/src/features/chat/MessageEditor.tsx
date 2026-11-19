import { useRef, useState } from "react";
import { reportError } from "../../lib/toast";
import { useFocusOnMount } from "../../lib/useFocusOnMount";
import { Button, Kbd, TextArea } from "../../ui";

const MAX_ROWS = 10;

/** Edits a message in place: Enter saves, Escape cancels. */
export function MessageEditor({
  initialText,
  onSave,
  onClose,
}: {
  initialText: string;
  onSave: (text: string) => Promise<void>;
  onClose: () => void;
}) {
  const [text, setText] = useState(initialText);
  const [isSaving, setSaving] = useState(false);
  const input = useRef<HTMLTextAreaElement>(null);
  useFocusOnMount(input);

  const save = async () => {
    if (!text.trim() || text.trim() === initialText.trim()) return onClose();
    setSaving(true);
    try {
      await onSave(text);
      onClose();
    } catch (error) {
      reportError(error);
      setSaving(false);
    }
  };

  return (
    <div className="mt-1 flex flex-col gap-1.5">
      <TextArea
        ref={input}
        aria-label="Edit message"
        value={text}
        rows={Math.min(MAX_ROWS, text.split("\n").length)}
        onChange={(event) => setText(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
            event.preventDefault();
            save();
          }
          if (event.key === "Escape") onClose();
        }}
      />
      <div className="flex items-center gap-2 text-[11px] text-faint">
        <Kbd>Enter</Kbd> to save · <Kbd>Esc</Kbd> to cancel
        <div className="ml-auto flex gap-1.5">
          <Button variant="ghost" size="sm" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" size="sm" isLoading={isSaving} onClick={save}>
            Save
          </Button>
        </div>
      </div>
    </div>
  );
}
