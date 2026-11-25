import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { appLink } from "./appLinks";
import { reportError } from "./toast";
import type { RestartOutcome } from "./updateFlow";

export type Session = { workspace_id: string; workspace_name: string; me: string; is_admin: boolean };
/** A workspace on this device, in the order the rail shows them. */
export type WorkspaceItem = {
  workspace_id: string;
  name: string;
  /** The workspace's picture; null shows its generated letter tile. */
  icon: string | null;
  /** Beyond the first few, a workspace checks the bucket less often (messages still arrive at once). */
  is_live: boolean;
  /** Muted on this device: no notifications from it. */
  is_muted: boolean;
};
/** The theme and accent an admin chose as the workspace's default; null for none. Names the app doesn't know mean none. */
export type WorkspaceLook = {
  theme: string | null;
  accent: string | null;
};
/** What a workspace's rail icon shows: the same counts as its sidebar. */
export type WorkspaceBadge = {
  workspace_id: string;
  unread: boolean;
  mentions: number;
  /** What needs you, as the window's badge counts it: Inbox outside DMs, unread unmuted DMs, due reminders. */
  attention: number;
};

/** The workspace the app shows; changes from the others aren't passed on. */
let selectedWorkspaceId: string | null = null;

function rememberSelected<T extends Session | null>(session: T): T {
  selectedWorkspaceId = session?.workspace_id ?? null;
  return session;
}
export type NotifyLevel = "all" | "mentions" | "nothing";
export type ChannelKind = "public" | "private" | "dm" | "notes";
/**
 * Who is in a private channel (member ids, earliest added first), and who owns it. `invited` are
 * members who haven't shown yet that they can read it: they never opened it, or never got its key.
 */
export type PrivateMembers = { members: string[]; invited: string[]; owner: string | null; is_owner: boolean };
export type Channel = {
  id: string;
  kind: ChannelKind;
  /** "" for a DM; show its members instead. */
  name: string;
  /** A DM's members other than you (member ids). */
  dm_members: string[];
  /** False once you left or were removed from a private channel. */
  is_member: boolean;
  /** You're in this private channel, but its owner started a newer key your device never got. */
  is_missing_key: boolean;
  can_delete: boolean;
  unread: number;
  unread_mentions: number;
  last_read: MessageCursor | null;
  notify: NotifyLevel;
};
export type Reaction = { emoji: string; count: number; mine: boolean };
export type Message = {
  id: string;
  channel: string;
  author: string;
  author_name: string;
  author_is_bot: boolean;
  text: string;
  reply_to: string | null;
  created_ms: number;
  lamport: number;
  is_edited: boolean;
  can_edit: boolean;
  can_delete: boolean;
  mentions_me: boolean;
  saved: SavedMark | null;
  reply_count: number;
  last_reply_ms: number | null;
  recent_reply_authors: string[];
  last_reply_author: string | null;
  last_reply_preview: string | null;
  is_following: boolean;
  unread_replies: number;
  reactions: Reaction[];
  /** Files shared with it, in the order they were sent. */
  files: FileInfo[];
};
/** A selection in a page as Yjs relative positions (`Y.encodeRelativePosition`), so it stays put as text moves. */
export type CursorSelection = { anchor: number[]; head: number[] };
/** Another member's cursor on a page: none while they aren't in its text. */
export type PageCursor = { member: string; selection: CursorSelection | null; idle_ms: number };
/** Who else is on a page, and their cursors. */
export type PagePresence = { viewers: string[]; cursors: PageCursor[] };

/** A shared file. Its bytes come through `api.fileBytes`; show it by what they are, never by its name or type. */
export type FileInfo = {
  id: string;
  /** The channel, DM or page it was shared in. */
  place: string;
  /** Null for a page's file. */
  message: string | null;
  author: string;
  name: string;
  /** As the sender's system named it; only a hint. */
  media_type: string;
  size: number;
  created_ms: number;
  can_delete: boolean;
  /** Whether you may rename it (you shared it). */
  can_rename: boolean;
};
/** A file picked, dropped or pasted, waiting to be sent; the token is all the app can send. */
export type StagedFile = { token: string; name: string; size: number; media_type: string };
export type InboxItem = {
  message: Message;
  channel_name: string;
  thread_root: string | null;
  reason: "mention" | "thread_reply";
};
export type SectionSort = "name" | "recent";
export type SidebarSection = {
  id: string;
  name: string;
  sort: SectionSort;
  is_collapsed: boolean;
  is_builtin: boolean;
  channels: string[];
};
export type LaterItem = {
  id: string;
  saved_ms: number;
  remind_ms: number | null;
  is_done: boolean;
  message: LinkedMessage | null;
  page: Page | null;
};
export type Draft = { target: string; channel: string; text: string; updated_ms: number };
export type SavedMark = { remind_ms: number | null; is_done: boolean };
export type LinkedMessage = { message: Message; thread_root: string | null };
/** A mention of you in a page, the latest from its author there. */
export type PageMention = {
  page: string;
  page_title: string;
  author: string;
  /** The page's line at the mention now, as plain text. */
  preview: string;
  /** Where the mention is, as an encoded Yjs relative position. */
  position: number[];
  mentioned_ms: number;
};
export type Inbox = { attention: InboxItem[]; channels: Channel[]; page_mentions: PageMention[] };
export type MessageCursor = { lamport: number; author: string };
export type Page = {
  id: string;
  parent: string | null;
  title: string;
  updated_ms: number;
  can_delete: boolean;
  is_saved: boolean;
};
/** One editing session's Yjs client id, bound to this member; `nonce` goes back with each of its updates. */
export type PageEditorSession = { client_id: number; nonce: string };
export type PresenceState = "online" | "away" | "unreachable";
export type MemberStatus = { emoji: string; text: string; until_ms: number | null; alerts: NotifyLevel };
/** One working interval in their time zone. `days`: bit 0 is Monday, bit 6 Sunday. An end not after the start crosses midnight. */
export type WorkingHours = { days: number; start_minute: number; end_minute: number };
/** What a member shares about themselves; an empty string is unset. */
export type ProfileDetails = {
  title: string;
  pronouns: string;
  bio: string;
  email: string;
  phone: string;
  website: string;
  location: string;
  time_zone: string;
  working_hours: WorkingHours | null;
};
export type Member = {
  id: string;
  name: string;
  is_bot: boolean;
  is_admin: boolean;
  active: boolean;
  status: MemberStatus | null;
  presence: PresenceState;
  details: ProfileDetails;
  /** Their picture's id, if they shared one; it changes with the picture. */
  picture: string | null;
  /** The code compared when they joined ("1234 5678 …"); it tells apart members who share a name. */
  safety_code: string;
};
export type SearchHit = {
  kind: "message" | "page";
  id: string;
  title: string;
  snippet: string;
  channel: string | null;
  thread_root: string | null;
  /** Who wrote a message; a page has no single author. */
  author: string | null;
  /** When a message was sent, or a page last changed. */
  time_ms: number;
};
export type InviteState = "open" | "requested" | "accepted" | "rejected" | "revoked" | "expired";
export type Invite = {
  token: string;
  label: string;
  created_ms: number;
  expires_ms: number;
  state: InviteState;
  requester_id: string | null;
  requester_name: string | null;
  requester_is_bot: boolean;
  requester_safety_code: string | null;
};
export type StorageSettings = {
  /** Every object in the bucket sits under this prefix, so bucket rules name it. */
  workspace_id: string;
  endpoint: string;
  bucket: string;
  region: string;
  access_key_id: string;
  /** Bytes in the admin's last recovery snapshot; null for members and before the admin made a kit. */
  recovery_snapshot_bytes: number | null;
  /** The largest recovery snapshot restore reads, in bytes. Past it, the kit can't restore the workspace. */
  recovery_snapshot_limit: number;
};
/**
 * Whether the bucket keeps deleted or overwritten data. "locked": deletes are refused, safe even from someone
 * with the keys. "versioned": accidents can be undone, but someone with the keys could delete old versions.
 * "unknown": never checked, or the last check failed.
 */
export type StorageProtection = "locked" | "versioned" | "unprotected" | "unknown";
export type ProtectionCheck = { state: StorageProtection; checked_ms: number | null };
/** The storage provider, told from the bucket's endpoint. */
export type StorageProvider = "scaleway" | "r2" | "b2" | "s3" | "other";
/** A member other than the admin, and whether their device now uses the new key (a hint only). */
export type MemberMove = { member: string; has_moved: boolean };
/** What the last check of the old key found, while it isn't proved dead. */
export type OldKeyState = "not_checked" | "still_works" | "unknown";
/** What checking the old key found: `dead` once every replaced key is refused. */
export type OldKeyCheck = "dead" | "still_works" | "unknown";
/** A change of bucket key under way, after a member was removed. Admin only. */
export type StorageRotation = {
  provider: StorageProvider;
  /** The key in use is one no removed member had. */
  has_new_key: boolean;
  members: MemberMove[];
  old_key: OldKeyState;
};
export type SyncStatus = {
  peers: number;
  last_sync_ms: number | null;
  error: string | null;
  removed: boolean;
  needs_update: boolean;
  /** Member ids of the teammates whose apps sent what this version can't read. */
  needs_update_from: string[];
};
export type BucketConfig = {
  endpoint: string;
  region: string;
  bucket: string;
  access_key_id: string;
  secret_access_key: string;
};

export type Change =
  | {
      kind: "alert";
      channel: string;
      channel_name: string;
      message: string;
      author: string;
      author_name: string;
      preview: string;
    }
  /** Someone mentioned you in a page just now, and your status allows alerts. */
  | {
      kind: "page_alert";
      page: string;
      page_title: string;
      author: string;
      author_name: string;
      preview: string;
      position: number[];
    }
  /** Unread mentions of you in pages changed. */
  | { kind: "page_mentions" }
  | { kind: "channels" }
  | { kind: "drafts" }
  | { kind: "later" }
  | { kind: "messages"; channel: string }
  | { kind: "pages" }
  | { kind: "doc"; page: string; update: number[] }
  | { kind: "page_reset"; page: string }
  | { kind: "page_files"; page: string }
  /** Who is on the page, or where their cursors are, changed. */
  | { kind: "page_presence"; page: string }
  | { kind: "members" }
  | { kind: "invites" }
  | { kind: "storage" }
  | { kind: "storage_protection" }
  /** The bucket key rotation (`storage_rotation`) changed. */
  | { kind: "key_rotation" }
  /** The workspaces on this device changed: added, left, moved, selected, renamed or given an icon, or run live or not. */
  | { kind: "workspaces" }
  /** The shown workspace's default theme or accent changed. */
  | { kind: "workspace_look" }
  /** How far this device's upload into `channel` has got, in bytes of the files. */
  | { kind: "upload"; channel: string; sent: number; total: number }
  | { kind: "sync" }
  | { kind: "removed" }
  /** Who is in `channel`'s huddle changed. */
  | { kind: "huddle"; channel: string }
  /** Something about this device's own huddle changed. */
  | { kind: "my_huddle" }
  /** Someone started a huddle in a DM you're in; raised once, and only if the DM may alert you. */
  | { kind: "huddle_started"; channel: string; member: string; member_name: string }
  /** An incoming huddle started ringing on this device; exactly one `huddle_ring_stopped` follows. */
  | { kind: "huddle_ringing"; channel: string; member: string; member_name: string }
  /** The ring for an incoming huddle stopped: answered, declined, the caller left, or nobody answered in time. */
  | { kind: "huddle_ring_stopped"; channel: string }
  /** Someone in this device's huddle started or stopped speaking; your own member id for your voice. */
  | { kind: "huddle_speaking"; member: string; speaking: boolean }
  /** The microphones and speakers offered, or the choice among them, changed. */
  | { kind: "huddle_devices" }
  /** Whether huddles play join and leave sounds on this device changed. */
  | { kind: "huddle_sounds" };

/** How the app shows itself outside its window on this device, and on which system. */
export type AppIcon = {
  platform: string;
  /** A menu bar item on macOS (Linux and Windows always have a tray icon). */
  is_in_menu_bar: boolean;
  /** The unread count on the Dock or taskbar icon. */
  shows_unread_count: boolean;
  /** The tray icon as a one-colour planet instead of the app icon (Linux, Windows). */
  is_tray_monochrome: boolean;
};

/** What a click on a desktop notification opens. */
export type AlertTarget =
  | { kind: "message" | "channel"; id: string }
  /** A spot in a page, as an encoded Yjs relative position. */
  | { kind: "page"; id: string; position: number[] };

/** Who is in a channel's huddle; you first when you're in it. */
export type Huddle = { channel: string; participants: HuddleParticipant[]; is_full: boolean };
export type HuddleParticipant = {
  member: string;
  camera: boolean;
  screen: boolean;
  is_me: boolean;
  /** Their app can't exchange audio with yours; one of you must update. */
  needs_update: boolean;
};
/** `joining` lasts only while joining runs; each peer's `link` shows the real wait. */
export type HuddlePhase = "joining" | "in";
/** `relayed` works, but may sound worse than `direct`. */
export type HuddleLink = "connecting" | "direct" | "relayed" | "reconnecting";
/** `off` means muted, with the microphone really closed; `silent` is open but delivering exact silence. */
export type MicState = "off" | "opening" | "live" | "permission_denied" | "silent" | "lost";
export type OutputState = "playing" | "bluetooth_call_mode" | "lost";
export type HuddlePeer = { member: string; link: HuddleLink; speaking: boolean };
/** Your huddle, in whichever workspace on this device holds it (one at a time). */
export type MyHuddle = {
  workspace_id: string;
  channel: string;
  /** "#general", or a DM's people joined by ", ": enough to name it while another workspace is shown. */
  channel_name: string;
  phase: HuddlePhase;
  muted: boolean;
  mic: MicState;
  output: OutputState;
  peers: HuddlePeer[];
};
/** The incoming huddle ringing on this device. */
export type IncomingHuddle = { workspace_id: string; channel: string; member: string; member_name: string };
export type AudioDevice = {
  id: string;
  name: string;
  is_default: boolean;
  /** Using its microphone drops it to call quality. */
  is_bluetooth: boolean;
};
/** `chosen` is null for the system's default, which is also what a chosen device that's gone shows as. */
export type AudioDevices = { inputs: AudioDevice[]; outputs: AudioDevice[]; chosen: AudioDeviceChoice };
/** null means the system's default. */
export type AudioDeviceChoice = { input: string | null; output: string | null };

export const api = {
  session: () => invoke<Session | null>("session").then(rememberSelected),
  createWorkspace: (workspaceName: string, name: string, bucket: BucketConfig) =>
    invoke<Session>("create_workspace", { workspaceName, name, bucket }).then(rememberSelected),
  joinWorkspace: (invite: string, name: string) =>
    invoke<Session>("join_workspace", { invite, name }).then(rememberSelected),
  /** Stops a join still waiting for the admin; `joinWorkspace` then fails. */
  cancelJoin: () => invoke<void>("cancel_join"),
  /** Shows how many things need you in the window title, the taskbar and the tray. */
  setAttentionCount: (count: number) => invoke<void>("set_attention_count", { count }),
  restoreWorkspace: (kit: string) => invoke<Session>("restore_workspace", { kit }).then(rememberSelected),
  workspaces: () => invoke<WorkspaceItem[]>("workspaces"),
  /** Shows another workspace; every other command then acts on it. */
  selectWorkspace: (workspaceId: string) => invoke<Session>("select_workspace", { workspaceId }).then(rememberSelected),
  moveWorkspace: (workspaceId: string, position: number) => invoke<void>("move_workspace", { workspaceId, position }),
  /** Deletes the workspace from this device for good; resolves to what to show next, if any. */
  leaveWorkspace: (workspaceId: string) =>
    invoke<Session | null>("leave_workspace", { workspaceId }).then(rememberSelected),
  workspaceBadges: () => invoke<WorkspaceBadge[]>("workspace_badges"),
  setWorkspaceMuted: (workspaceId: string, muted: boolean) =>
    invoke<void>("set_workspace_muted", { workspaceId, muted }),
  markWorkspaceRead: (workspaceId: string) => invoke<void>("mark_workspace_read", { workspaceId }),
  workspaceLook: () => invoke<WorkspaceLook>("workspace_look"),
  /** Admin only; null clears a default. */
  setWorkspaceLook: (theme: string | null, accent: string | null) =>
    invoke<void>("set_workspace_look", { theme, accent }),
  /** Empty if it has none. */
  workspaceIcon: async (workspaceId: string) =>
    new Uint8Array(await invoke<ArrayBuffer>("workspace_icon", { workspaceId })),
  /** Admin only: an already cropped PNG, JPEG or WebP; null goes back to the letter. */
  setWorkspaceIcon: (image: Uint8Array | null) =>
    invoke<void>("set_workspace_icon", { image: image ? Array.from(image) : [] }),
  /** Holds every workspace's notifications until then; null lets them show again. */
  setNotificationsPaused: (untilMs: number | null) => invoke<void>("set_notifications_paused", { untilMs }),
  notificationsPaused: () => invoke<number | null>("notifications_paused"),
  appIcon: () => invoke<AppIcon>("app_icon"),
  setInMenuBar: (isShown: boolean) => invoke<void>("set_in_menu_bar", { isShown }),
  setShowsUnreadCount: (isShown: boolean) => invoke<void>("set_shows_unread_count", { isShown }),
  setTrayMonochrome: (isMonochrome: boolean) => invoke<void>("set_tray_monochrome", { isMonochrome }),
  testBucket: (bucket: BucketConfig) => invoke<void>("test_bucket", { bucket }),
  storageSettings: () => invoke<StorageSettings>("storage_settings"),
  /** The last check's result, without asking the bucket. */
  storageProtection: () => invoke<ProtectionCheck>("storage_protection"),
  /** Checks the bucket now (takes a few seconds). */
  checkStorageProtection: () => invoke<ProtectionCheck>("check_storage_protection"),
  /** Resolves to the admin's new recovery kit, if they have one. */
  updateStorage: (region: string, accessKeyId: string, secretAccessKey: string) =>
    invoke<string | null>("update_storage", { region, accessKeyId, secretAccessKey }),
  /** The bucket key change under way, or null when none is due. Admin only. */
  storageRotation: () => invoke<StorageRotation | null>("storage_rotation"),
  /** Tries the old keys; `dead` ends the change unless the key in use is a removed member's too. */
  checkOldKeyDeleted: () => invoke<OldKeyCheck>("check_old_key_deleted"),
  /** Ends the change without proving the old key dead, and forgets which keys were compromised. */
  cancelStorageRotation: () => invoke<void>("cancel_storage_rotation"),
  safetyCode: (invite: string) => invoke<string>("safety_code", { invite }),
  recoveryKit: () => invoke<string>("recovery_kit"),
  channels: () => invoke<Channel[]>("channels"),
  createChannel: (name: string) => invoke<string>("create_channel", { name }),
  createPrivateChannel: (name: string, members: string[]) =>
    invoke<string>("create_private_channel", { name, members }),
  /** The DM with exactly these members and you; opening it again finds the same one. */
  openDm: (members: string[]) => invoke<string>("open_dm", { members }),
  /** Your notes to yourself, made the first time. */
  openNotes: () => invoke<string>("open_notes"),
  privateMembers: (channel: string) => invoke<PrivateMembers>("private_members", { channel }),
  addToPrivateChannel: (channel: string, member: string) => invoke<void>("add_to_private_channel", { channel, member }),
  removeFromPrivateChannel: (channel: string, member: string) =>
    invoke<void>("remove_from_private_channel", { channel, member }),
  handOverPrivateChannel: (channel: string, member: string) =>
    invoke<void>("hand_over_private_channel", { channel, member }),
  leavePrivateChannel: (channel: string) => invoke<void>("leave_private_channel", { channel }),
  rotatePrivateChannelKey: (channel: string) => invoke<void>("rotate_private_channel_key", { channel }),
  renameChannel: (channel: string, name: string) => invoke<void>("rename_channel", { channel, name }),
  deleteChannel: (channel: string) => invoke<void>("delete_channel", { channel }),
  markRead: (channel: string, message: string) => invoke<void>("mark_read", { channel, message }),
  thread: (root: string) => invoke<Message[]>("thread", { root }),
  inbox: () => invoke<Inbox>("inbox"),
  drafts: () => invoke<Draft[]>("drafts"),
  /** Your own messages, newest first; `text` narrows them, `before` pages back from the oldest shown. */
  sentMessages: (text: string, before: MessageCursor | null, limit: number) =>
    invoke<LinkedMessage[]>("sent_messages", { text, before, limit }),
  later: () => invoke<LaterItem[]>("later"),
  sidebarSections: () => invoke<SidebarSection[]>("sidebar_sections"),
  createSection: (name: string) => invoke<string>("create_section", { name }),
  renameSection: (section: string, name: string) => invoke<void>("rename_section", { section, name }),
  deleteSection: (section: string) => invoke<void>("delete_section", { section }),
  setSectionSort: (section: string, sort: SectionSort) => invoke<void>("set_section_sort", { section, sort }),
  setSectionCollapsed: (section: string, collapsed: boolean) =>
    invoke<void>("set_section_collapsed", { section, collapsed }),
  moveChannel: (channel: string, section: string | null) => invoke<void>("move_channel", { channel, section }),
  saveForLater: (item: string, remindMs: number | null) => invoke<void>("save_for_later", { item, remindMs }),
  removeFromLater: (item: string) => invoke<void>("remove_from_later", { item }),
  markLaterDone: (item: string, done: boolean) => invoke<void>("mark_later_done", { item, done }),
  /** Due reminders from every workspace on this device, each marked fired. */
  takeDueReminders: () => invoke<{ workspace_id: string; item: LaterItem }[]>("take_due_reminders"),
  /** When the next reminder comes due, in any workspace. */
  nextReminderMs: () => invoke<number | null>("next_reminder_ms"),
  setDraft: (target: string, text: string) => invoke<void>("set_draft", { target, text }),
  linkedMessage: (id: string) => invoke<LinkedMessage | null>("linked_message", { id }),
  markAllRead: () => invoke<void>("mark_all_read"),
  markPageMentionsRead: (page: string) => invoke<void>("mark_page_mentions_read", { page }),
  setFollowing: (root: string, following: boolean) => invoke<void>("set_following", { root, following }),
  markThreadRead: (root: string, message: string) => invoke<void>("mark_thread_read", { root, message }),
  setNotifyLevel: (channel: string, level: NotifyLevel) => invoke<void>("set_notify_level", { channel, level }),
  /** A desktop notification; a click on it opens `target` (sent back as an "alert-opened" event). */
  /** `picture` is PNG bytes shown with the notification, named for the member it shows. */
  showAlert: (
    workspaceId: string,
    title: string,
    body: string,
    target: AlertTarget | null,
    picture: { member: string; png: number[] } | null,
  ) => invoke<void>("show_alert", { workspaceId, title, body, target, picture }),
  messages: (channel: string, before: MessageCursor | null = null, limit = 100) =>
    invoke<Message[]>("messages", { channel, before, limit }),
  sendMessage: (channel: string, text: string, replyTo: string | null) =>
    invoke<string>("send_message", { channel, text, replyTo }),
  editMessage: (message: string, text: string) => invoke<void>("edit_message", { message, text }),
  deleteMessage: (message: string) => invoke<void>("delete_message", { message }),
  react: (message: string, emoji: string, active: boolean) => invoke<void>("react", { message, emoji, active }),
  pages: () => invoke<Page[]>("pages"),
  createPage: (title: string, parent: string | null) => invoke<string>("create_page", { title, parent }),
  renamePage: (page: string, title: string) => invoke<void>("rename_page", { page, title }),
  deletePage: (page: string) => invoke<void>("delete_page", { page }),
  pageState: async (page: string) => new Uint8Array(await invoke<ArrayBuffer>("page_state", { page })),
  pageEditorSession: (page: string) => invoke<PageEditorSession>("page_editor_session", { page }),
  applyDocUpdate: (page: string, nonce: string, update: Uint8Array) =>
    invoke<void>("apply_doc_update", { page, nonce, update: Array.from(update) }),
  members: () => invoke<Member[]>("members"),
  setProfile: (name: string) => invoke<void>("set_profile", { name }),
  setProfileDetails: (details: ProfileDetails) => invoke<void>("set_profile_details", { details }),
  setStatus: (emoji: string, text: string, untilMs: number | null, alerts: NotifyLevel) =>
    invoke<void>("set_status", { emoji, text, untilMs, alerts }),
  clearStatus: () => invoke<void>("clear_status"),
  setAway: (away: boolean) => invoke<void>("set_away", { away }),
  appearOffline: () => invoke<boolean>("appear_offline"),
  setAppearOffline: (offline: boolean) => invoke<void>("set_appear_offline", { offline }),
  removeMember: (member: string) => invoke<void>("remove_member", { member }),
  invites: () => invoke<Invite[]>("invites"),
  createInvite: (label: string) => invoke<string>("create_invite", { label }),
  approveInvite: (token: string) => invoke<void>("approve_invite", { token }),
  rejectInvite: (token: string) => invoke<void>("reject_invite", { token }),
  revokeInvite: (token: string) => invoke<void>("revoke_invite", { token }),
  search: (query: string) => invoke<SearchHit[]>("search", { query }),
  /** Opens the system's file picker; resolves to what was picked (nothing if cancelled). */
  pickFiles: () => invoke<StagedFile[]>("pick_files"),
  /** The file crosses the bridge as raw bytes, framed by `framePastedFile`. */
  stagePastedFile: async (file: File) =>
    invoke<StagedFile>(
      "stage_pasted_file",
      framePastedFile(file.name, file.type, new Uint8Array(await file.arrayBuffer())),
    ),
  /** The system clipboard's image: width and height (little-endian u32 each), then RGBA pixels. */
  clipboardImage: async () => new Uint8Array(await invoke<ArrayBuffer>("clipboard_image")),
  unstageFile: (token: string) => invoke<void>("unstage_file", { token }),
  /** A staged image's bytes, for its thumbnail; refused for anything over the inline image limit. */
  stagedFileBytes: async (token: string) => new Uint8Array(await invoke<ArrayBuffer>("staged_file_bytes", { token })),
  /** Uploads the staged files, then sends one message carrying them; `text` may be empty. */
  sendFiles: (channel: string, text: string, replyTo: string | null, tokens: string[]) =>
    invoke<string>("send_files", { channel, text, replyTo, tokens }),
  fileBytes: async (file: string) => new Uint8Array(await invoke<ArrayBuffer>("file_bytes", { file })),
  /** Asks where to save it, then saves; false if cancelled. */
  saveFile: (file: string) => invoke<boolean>("save_file", { file }),
  deleteFile: (file: string) => invoke<void>("delete_file", { file }),
  renameFile: (file: string, name: string) => invoke<void>("rename_file", { file, name }),
  files: () => invoke<FileInfo[]>("files"),
  /** Uploads a staged file into a page; resolves to its id, for the page's text to link. */
  sharePageFile: (page: string, token: string) => invoke<string>("share_page_file", { page, token }),
  pageFiles: (page: string) => invoke<FileInfo[]>("page_files", { page }),
  /** Starts hearing who is on the page and where their cursors are; with `share`, they hear you too. */
  joinPage: (page: string, share: boolean) => invoke<void>("join_page", { page, share }),
  leavePage: (page: string) => invoke<void>("leave_page", { page }),
  /** Your selection on a page you joined sharing, or null when you're not in its text. */
  moveCursor: (page: string, selection: CursorSelection | null) => invoke<void>("move_cursor", { page, selection }),
  pagePresence: (page: string) => invoke<PagePresence>("page_presence", { page }),
  /** A page's own file only; any other is refused, so page text can't pull one in by id. */
  pageFileBytes: async (page: string, file: string) =>
    new Uint8Array(await invoke<ArrayBuffer>("page_file_bytes", { page, file })),
  /** Asks where to save one of a page's files, then saves; false if cancelled. */
  savePageFile: (page: string, file: string) => invoke<boolean>("save_page_file", { page, file }),
  /** Empty if they have none. */
  profilePicture: async (member: string) => new Uint8Array(await invoke<ArrayBuffer>("profile_picture", { member })),
  /** An already cropped PNG, JPEG or WebP; null removes yours. */
  setProfilePicture: (image: Uint8Array | null) =>
    invoke<void>("set_profile_picture", { image: image ? Array.from(image) : [] }),
  status: () => invoke<SyncStatus>("status"),
  huddle: (channel: string) => invoke<Huddle>("huddle", { channel }),
  myHuddle: () => invoke<MyHuddle | null>("my_huddle"),
  /** Leaves any other huddle first. */
  joinHuddle: (channel: string, muted: boolean) => invoke<void>("join_huddle", { channel, muted }),
  leaveHuddle: () => invoke<void>("leave_huddle"),
  /** Stops the ring for an incoming huddle without joining it. */
  declineHuddle: (workspaceId: string) => invoke<void>("decline_huddle", { workspaceId }),
  /** The version downloaded and ready to install, or null while there's nothing newer. */
  appVersion: getVersion,
  checkForUpdate: () => invoke<string | null>("check_for_update"),
  /** Installs the ready update and restarts into it, after uploading unsent changes; refused during a huddle. */
  restartToUpdate: () => invoke<RestartOutcome>("restart_to_update"),
  /** The incoming huddle ringing now, if any. */
  huddleRinging: () => invoke<IncomingHuddle | null>("huddle_ringing"),
  setHuddleMuted: (muted: boolean) => invoke<void>("set_huddle_muted", { muted }),
  huddleDevices: () => invoke<AudioDevices>("huddle_devices"),
  setHuddleDevices: (choice: AudioDeviceChoice) => invoke<void>("set_huddle_devices", { choice }),
  /** Whether huddles play join and leave sounds on this device. */
  huddleSounds: () => invoke<boolean>("huddle_sounds"),
  setHuddleSounds: (isOn: boolean) => invoke<void>("set_huddle_sounds", { isOn }),
};

/**
 * A pasted file as one byte string: its name and media type, each as a
 * 2-byte big-endian length and UTF-8, then the file's bytes. Must match
 * `unframe_pasted_file` in src-tauri/src/files.rs.
 */
export function framePastedFile(name: string, mediaType: string, bytes: Uint8Array): Uint8Array {
  const encoder = new TextEncoder();
  const parts = [encoder.encode(name.slice(0, 1000)), encoder.encode(mediaType.slice(0, 200))];
  const framed = new Uint8Array(parts.reduce((sum, part) => sum + 2 + part.length, 0) + bytes.length);
  let offset = 0;
  for (const part of parts) {
    framed[offset] = part.length >> 8;
    framed[offset + 1] = part.length & 0xff;
    framed.set(part, offset + 2);
    offset += 2 + part.length;
  }
  framed.set(bytes, offset);
  return framed;
}

type ChangeListener = (change: Change) => void;
const listeners = new Set<ChangeListener>();
let isListening = false;
/** Listeners for every workspace's changes, each told which workspace it came from. */
const everyWorkspaceListeners = new Set<(workspaceId: string | null, change: Change) => void>();

function listenOnce() {
  if (isListening) return;
  isListening = true;
  listen<{ workspace_id: string | null; change: Change }>("change", (event) => {
    const { workspace_id: workspaceId, change } = event.payload;
    for (const notify of everyWorkspaceListeners) notify(workspaceId, change);
    if (workspaceId !== null && workspaceId !== selectedWorkspaceId) return;
    for (const notify of listeners) notify(change);
  }).catch(reportError);
}

/** One native subscription, fanned out to every component that cares. Only the shown workspace's changes, and the app's own. */
export function onChange(listener: ChangeListener): () => void {
  listenOnce();
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Changes from every workspace on this device, for what spans them: the rail's badges and notifications. */
export function onAnyWorkspaceChange(listener: (workspaceId: string | null, change: Change) => void): () => void {
  listenOnce();
  everyWorkspaceListeners.add(listener);
  return () => everyWorkspaceListeners.delete(listener);
}

/** The workspace the app shows, as the last session said. */
export function shownWorkspaceId(): string | null {
  return selectedWorkspaceId;
}

/** Files dropped on the window, already staged; and whether files are being dragged over it. */
/** A point in the window, in page pixels. */
export type DropPoint = { x: number; y: number };
/** Files dropped on the window, staged, and where: null when the window couldn't say. */
export type FilesDropped = { files: StagedFile[]; position: DropPoint | null };

export function onFilesDropped(listener: (drop: FilesDropped) => void): () => void {
  return subscribe<FilesDropped>("files-dropped", listener);
}
/** Where files are being dragged over the window; null once the drag leaves or drops. */
export function onFileDrag(listener: (position: DropPoint | null) => void): () => void {
  return subscribe<DropPoint | null>("file-drag", listener);
}

function subscribe<T>(event: string, listener: (payload: T) => void): () => void {
  let isActive = true;
  let stop: (() => void) | null = null;
  listen<T>(event, (received) => {
    if (isActive) listener(received.payload);
  })
    .then((unlisten) => {
      if (isActive) stop = unlisten;
      else unlisten();
    })
    .catch(reportError);
  return () => {
    isActive = false;
    stop?.();
  };
}

export const messageLink = (workspaceId: string, id: string) => appLink(workspaceId, "message", id);
export const pageLink = (workspaceId: string, id: string) => appLink(workspaceId, "page", id);
