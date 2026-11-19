import { LOCAL_SHORTCUTS, SHORTCUTS } from "../../lib/shortcuts";
import { Dialog, Kbd } from "../../ui";

export function ShortcutsDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title="Keyboard shortcuts" width={420}>
      <ul className="flex flex-col gap-1 p-5 pt-3">
        {[...SHORTCUTS, ...LOCAL_SHORTCUTS].map((shortcut) => (
          <li key={shortcut.description} className="flex items-center justify-between gap-4 py-1 text-[13px]">
            <span>{shortcut.description}</span>
            <span className="flex gap-1">
              {shortcut.keys.map((key) => (
                <Kbd key={key}>{key}</Kbd>
              ))}
            </span>
          </li>
        ))}
      </ul>
    </Dialog>
  );
}
