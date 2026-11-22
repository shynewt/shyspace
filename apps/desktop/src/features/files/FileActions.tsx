import { Download, Ellipsis, MessageSquareText, Pencil, Trash2 } from "lucide-react";
import { useState } from "react";
import { api, type FileInfo } from "../../lib/api";
import { reportError } from "../../lib/toast";
import { useWorkspace } from "../../lib/workspace";
import { ConfirmDialog, IconButton, Menu, type MenuAction } from "../../ui";
import { RenameFileDialog } from "./RenameFileDialog";

/**
 * A file's buttons: "Show in chat" where asked, Save, and a menu that also
 * holds Rename and Delete for those who may. Pass `isMenuOpen` and
 * `onMenuOpenChange` to open the menu from elsewhere, like a right-click.
 */
export function FileActions({
  file,
  hasShowInChat,
  isMenuOpen,
  onMenuOpenChange,
  onLeave,
}: {
  file: FileInfo;
  /** Off where the file is already shown in its message. */
  hasShowInChat: boolean;
  isMenuOpen?: boolean;
  onMenuOpenChange?: (open: boolean) => void;
  /** Called before "Show in chat" takes you to the message, e.g. to close the viewer. */
  onLeave?: () => void;
}) {
  const { navigate } = useWorkspace();
  const [isConfirmingDelete, setConfirmingDelete] = useState(false);
  const [isRenaming, setRenaming] = useState(false);
  const message = hasShowInChat ? file.message : null;
  const showInChat = message
    ? () => {
        onLeave?.();
        navigate({ kind: "channel", id: file.place, focus: message });
      }
    : null;
  const save = () => {
    api.saveFile(file.id).catch(reportError);
  };

  const menu: MenuAction[] = [
    ...(showInChat ? [{ label: "Show in chat", icon: <MessageSquareText size={14} />, onSelect: showInChat }] : []),
    { label: "Save…", icon: <Download size={14} />, onSelect: save },
    ...(file.can_rename ? [{ label: "Rename…", icon: <Pencil size={14} />, onSelect: () => setRenaming(true) }] : []),
    ...(file.can_delete
      ? [
          {
            label: "Delete…",
            icon: <Trash2 size={14} />,
            onSelect: () => setConfirmingDelete(true),
            isDestructive: true,
          },
        ]
      : []),
  ];

  return (
    <>
      {showInChat && (
        <IconButton label={`Show ${file.name} in chat`} icon={<MessageSquareText size={15} />} onClick={showInChat} />
      )}
      <IconButton label={`Save ${file.name}`} icon={<Download size={15} />} onClick={save} />
      <Menu
        open={isMenuOpen}
        onOpenChange={onMenuOpenChange}
        trigger={<IconButton label={`More actions for ${file.name}`} icon={<Ellipsis size={15} />} />}
        items={menu}
      />
      {file.can_rename && <RenameFileDialog file={file} open={isRenaming} onOpenChange={setRenaming} />}
      <ConfirmDialog
        open={isConfirmingDelete}
        onOpenChange={setConfirmingDelete}
        title="Delete this file?"
        description="It disappears for everyone and leaves the workspace's storage. Anyone who already saved a copy keeps it. This can't be undone."
        confirmLabel="Delete file"
        onConfirm={() => api.deleteFile(file.id)}
      >
        <p className="selectable truncate text-sm">{file.name}</p>
      </ConfirmDialog>
    </>
  );
}
