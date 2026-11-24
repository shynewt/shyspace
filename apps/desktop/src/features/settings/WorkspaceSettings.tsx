import { ImageUp, Type } from "lucide-react";
import { useRef, useState } from "react";
import { api } from "../../lib/api";
import { useWorkspaceIcon } from "../../lib/files";
import { PALETTE_NAMES, PALETTE_SWATCHES } from "../../lib/palettes";
import { squarePicture } from "../../lib/picture";
import { PALETTES, workspaceDefaults } from "../../lib/preferences";
import { reportError, showToast } from "../../lib/toast";
import { useWorkspace } from "../../lib/workspace";
import { Avatar, Button, ThemeSwatches } from "../../ui";
import { useWorkspaces } from "../shell/Workspaces";
import { AccentSwatches } from "./AccentSwatches";
import { ManagedByAdmin, Setting } from "./layout";

/** The workspace's icon and default look: the admin sets them, everyone else sees them. */
export function WorkspaceSettings() {
  const { session } = useWorkspace();
  const { look } = useWorkspaces();
  const defaults = workspaceDefaults(look);
  const isAdmin = session.is_admin;
  // Unknown names are kept as they are, so saving one half doesn't erase the other's.
  const saveLook = (theme: string | null, accent: string | null) =>
    api.setWorkspaceLook(theme, accent).catch(reportError);
  return (
    <>
      {!isAdmin && <ManagedByAdmin>Anyone can still pick their own theme and accent in Appearance.</ManagedByAdmin>}
      <Setting title="Icon" description="Shown in the rail beside the sidebar. A square picture works best.">
        <WorkspaceIconField isAdmin={isAdmin} />
      </Setting>
      <Setting
        title="Default theme"
        description="What members see until they pick their own. Never a lock: anyone can change it for themselves."
      >
        <ThemeSwatches
          label="Default theme"
          options={PALETTES.map((palette) => ({
            value: palette,
            label: PALETTE_NAMES[palette],
            ...PALETTE_SWATCHES[palette],
          }))}
          value={defaults.palette}
          disabled={!isAdmin}
          onChange={(palette) => saveLook(palette === "shy" ? null : palette, look?.accent ?? null)}
        />
      </Setting>
      <Setting title="Default accent colour" description="Also a default only.">
        <AccentSwatches
          label="Default accent colour"
          value={defaults.accent}
          disabled={!isAdmin}
          onChange={(accent) => saveLook(look?.theme ?? null, accent === "violet" ? null : accent)}
        />
      </Setting>
    </>
  );
}

function WorkspaceIconField({ isAdmin }: { isAdmin: boolean }) {
  const { session } = useWorkspace();
  const { workspaces } = useWorkspaces();
  const icon = workspaces.find((workspace) => workspace.workspace_id === session.workspace_id)?.icon ?? null;
  const iconUrl = useWorkspaceIcon(session.workspace_id, icon);
  const input = useRef<HTMLInputElement>(null);
  const [isBusy, setBusy] = useState(false);

  const share = async (image: Uint8Array | null) => {
    setBusy(true);
    try {
      await api.setWorkspaceIcon(image);
      showToast(image ? "The workspace's picture is updated for everyone." : "The workspace shows its letter again.");
    } catch (error) {
      reportError(error);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex items-center gap-3">
      <Avatar id={session.workspace_id} name={session.workspace_name} size={48} pictureUrl={iconUrl} />
      {isAdmin && (
        <>
          <Button size="sm" icon={<ImageUp size={14} />} isLoading={isBusy} onClick={() => input.current?.click()}>
            {icon ? "Change picture" : "Upload picture…"}
          </Button>
          {icon && (
            <Button size="sm" variant="ghost" icon={<Type size={14} />} disabled={isBusy} onClick={() => share(null)}>
              Use the letter
            </Button>
          )}
          <input
            ref={input}
            type="file"
            accept="image/png,image/jpeg,image/webp,image/heic"
            aria-label="Choose a workspace picture"
            className="hidden"
            onChange={async (event) => {
              const file = event.target.files?.[0];
              event.target.value = "";
              if (!file) return;
              try {
                await share(await squarePicture(file));
              } catch (error) {
                reportError(error);
              }
            }}
          />
        </>
      )}
    </div>
  );
}
