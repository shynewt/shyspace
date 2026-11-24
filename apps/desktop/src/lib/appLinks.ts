/**
 * App links: `shy://<workspace id>/<kind>/<id>`, naming the workspace their
 * target is in. Links written before that, `shy://<kind>/<id>`, mean the
 * workspace they're read in. Mirrors `crates/shy-core/src/links.rs`.
 */

/** Exactly what the core makes a workspace id: 16 lowercase hex characters. */
const WORKSPACE_ID = "[0-9a-f]{16}";
const PAGE_ID = "[0-9a-f]+";
/** A message id; in a private channel or DM it starts with the channel, like `p:…/…`. */
const MESSAGE_ID = "(?:(?:p|dm):[0-9a-f]+/)?[0-9a-f]+";
const MEMBER_ID = "[0-9a-f]+";

const ID_PATTERNS = {
  page: PAGE_ID,
  message: MESSAGE_ID,
  member: MEMBER_ID,
  mention: "(?:channel|here)",
} as const;

export type AppLinkKind = keyof typeof ID_PATTERNS;

export type AppLink = {
  /** Null for a link written before links named their workspace. */
  workspace: string | null;
  kind: AppLinkKind;
  id: string;
};

/** Where an app link of `kind` is in text: the workspace (if named) in group 1, the id in group 2. */
export function appLinkPattern(kind: AppLinkKind, flags = "g"): RegExp {
  return new RegExp(`shy://(?:(${WORKSPACE_ID})/)?${kind}/(${ID_PATTERNS[kind]})`, flags);
}

const WHOLE = Object.fromEntries(
  Object.entries(ID_PATTERNS).map(([kind, id]) => [kind, new RegExp(`^shy://(?:(${WORKSPACE_ID})/)?${kind}/(${id})$`)]),
) as Record<AppLinkKind, RegExp>;

/** `url` read as an app link, if it is exactly one. */
export function parseAppLink(url: string): AppLink | null {
  for (const kind of Object.keys(WHOLE) as AppLinkKind[]) {
    const match = url.match(WHOLE[kind]);
    if (match) return { workspace: match[1] ?? null, kind, id: match[2] };
  }
  return null;
}

/** Whether `link` points into `workspaceId`, the workspace it's read in. */
export function isInWorkspace(link: Pick<AppLink, "workspace">, workspaceId: string): boolean {
  return link.workspace === null || link.workspace === workspaceId;
}

/**
 * A link to `id` of `kind` in `workspaceId`, as everything the app writes. A
 * workspace whose id isn't shaped like the core's (one joined with an odd
 * invite) gets an older link, which still means where it's read.
 */
export function appLink(workspaceId: string, kind: AppLinkKind, id: string): string {
  return IS_WORKSPACE_ID.test(workspaceId) ? `shy://${workspaceId}/${kind}/${id}` : `shy://${kind}/${id}`;
}

const IS_WORKSPACE_ID = new RegExp(`^${WORKSPACE_ID}$`);

/** What reading links needs to know of the workspaces on this device. */
export type WorkspaceLinks = {
  /** The workspace the text is read in. */
  workspaceId: string;
  /** Another workspace on this device, as its rail shows it; undefined if this device isn't in it. */
  workspaceOf: (workspaceId: string) => { name: string; icon: string | null } | undefined;
  /** Opens a page or message link of another workspace on this device, there. */
  openElsewhere: (link: AppLink) => void;
};
