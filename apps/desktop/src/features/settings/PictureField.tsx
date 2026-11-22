import { useRef, useState } from "react";
import { api, type Member } from "../../lib/api";
import { usePicture } from "../../lib/files";
import { displayName } from "../../lib/people";
import { squarePicture } from "../../lib/picture";
import { reportError, showToast } from "../../lib/toast";
import { Avatar, Button } from "../../ui";

/** Your picture: chosen, cropped to a square here, and shared at once. */
export function PictureField({ me }: { me: Member }) {
  const input = useRef<HTMLInputElement>(null);
  const [isBusy, setBusy] = useState(false);
  const pictureUrl = usePicture(me.id, me.picture);

  const share = async (image: Uint8Array | null) => {
    setBusy(true);
    try {
      await api.setProfilePicture(image);
      showToast(image ? "Your picture is updated for everyone." : "Your picture is removed.");
    } catch (error) {
      reportError(error);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex items-center gap-4">
      <Avatar id={me.id} name={displayName(me.name, me.id)} size={64} pictureUrl={pictureUrl} />
      <div className="flex flex-col gap-1.5">
        <div className="flex gap-2">
          <Button variant="secondary" size="sm" isLoading={isBusy} onClick={() => input.current?.click()}>
            {me.picture ? "Change picture" : "Add a picture"}
          </Button>
          {me.picture && (
            <Button variant="ghost" size="sm" disabled={isBusy} onClick={() => share(null)}>
              Remove
            </Button>
          )}
        </div>
        <p className="text-xs text-faint">
          Everyone in the workspace sees it. Location and camera details are removed.
        </p>
      </div>
      <input
        ref={input}
        type="file"
        accept="image/png,image/jpeg,image/webp,image/heic"
        aria-label="Choose a picture"
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
    </div>
  );
}
