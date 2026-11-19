import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { reportError } from "./toast";

export type Session = { workspace_name: string; me: string; is_admin: boolean };
export type NotifyLevel = "all" | "mentions" | "nothing";
export type Channel = {
  id: string;
  name: string;
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
  reply_count: number;
  last_reply_ms: number | null;
  is_following: boolean;
  unread_replies: number;
  reactions: Reaction[];
};
export type InboxItem = {
  message: Message;
  channel_name: string;
  thread_root: string | null;
  reason: "mention" | "thread_reply";
};
export type LinkedMessage = { message: Message; thread_root: string | null };
export type Inbox = { attention: InboxItem[]; channels: Channel[] };
export type MessageCursor = { lamport: number; author: string };
export type Page = { id: string; parent: string | null; title: string; updated_ms: number; can_delete: boolean };
export type Member = { id: string; name: string; is_bot: boolean; is_admin: boolean; active: boolean };
export type SearchHit = {
  kind: "message" | "page";
  id: string;
  title: string;
  snippet: string;
  channel: string | null;
  thread_root: string | null;
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
export type StorageSettings = { endpoint: string; bucket: string; region: string; access_key_id: string };
export type SyncStatus = {
  peers: number;
  last_sync_ms: number | null;
  error: string | null;
  removed: boolean;
  needs_update: boolean;
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
  | { kind: "channels" }
  | { kind: "messages"; channel: string }
  | { kind: "pages" }
  | { kind: "doc"; page: string; update: number[] }
  | { kind: "members" }
  | { kind: "invites" }
  | { kind: "storage" }
  | { kind: "sync" }
  | { kind: "removed" };

export const api = {
  session: () => invoke<Session | null>("session"),
  createWorkspace: (workspaceName: string, name: string, bucket: BucketConfig) =>
    invoke<Session>("create_workspace", { workspaceName, name, bucket }),
  joinWorkspace: (invite: string, name: string) => invoke<Session>("join_workspace", { invite, name }),
  restoreWorkspace: (kit: string) => invoke<Session>("restore_workspace", { kit }),
  testBucket: (bucket: BucketConfig) => invoke<void>("test_bucket", { bucket }),
  storageSettings: () => invoke<StorageSettings>("storage_settings"),
  /** Resolves to the admin's new recovery kit, if they have one. */
  updateStorage: (region: string, accessKeyId: string, secretAccessKey: string) =>
    invoke<string | null>("update_storage", { region, accessKeyId, secretAccessKey }),
  safetyCode: () => invoke<string>("safety_code"),
  recoveryKit: () => invoke<string>("recovery_kit"),
  channels: () => invoke<Channel[]>("channels"),
  createChannel: (name: string) => invoke<string>("create_channel", { name }),
  renameChannel: (channel: string, name: string) => invoke<void>("rename_channel", { channel, name }),
  deleteChannel: (channel: string) => invoke<void>("delete_channel", { channel }),
  markRead: (channel: string, message: string) => invoke<void>("mark_read", { channel, message }),
  thread: (root: string) => invoke<Message[]>("thread", { root }),
  inbox: () => invoke<Inbox>("inbox"),
  linkedMessage: (id: string) => invoke<LinkedMessage | null>("linked_message", { id }),
  markAllRead: () => invoke<void>("mark_all_read"),
  setFollowing: (root: string, following: boolean) => invoke<void>("set_following", { root, following }),
  markThreadRead: (root: string, message: string) => invoke<void>("mark_thread_read", { root, message }),
  setNotifyLevel: (channel: string, level: NotifyLevel) => invoke<void>("set_notify_level", { channel, level }),
  showAlert: (title: string, body: string) => invoke<void>("show_alert", { title, body }),
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
  applyDocUpdate: (page: string, update: Uint8Array) =>
    invoke<void>("apply_doc_update", { page, update: Array.from(update) }),
  members: () => invoke<Member[]>("members"),
  removeMember: (member: string) => invoke<void>("remove_member", { member }),
  invites: () => invoke<Invite[]>("invites"),
  createInvite: (label: string) => invoke<string>("create_invite", { label }),
  approveInvite: (token: string) => invoke<void>("approve_invite", { token }),
  rejectInvite: (token: string) => invoke<void>("reject_invite", { token }),
  revokeInvite: (token: string) => invoke<void>("revoke_invite", { token }),
  search: (query: string) => invoke<SearchHit[]>("search", { query }),
  status: () => invoke<SyncStatus>("status"),
};

type ChangeListener = (change: Change) => void;
const listeners = new Set<ChangeListener>();
let isListening = false;

/** One native subscription, fanned out to every component that cares. */
export function onChange(listener: ChangeListener): () => void {
  if (!isListening) {
    isListening = true;
    listen<Change>("change", (event) => {
      for (const notify of listeners) notify(event.payload);
    }).catch(reportError);
  }
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export const PAGE_LINK_PREFIX = "shy://page/";
export const MESSAGE_LINK_PREFIX = "shy://message/";
export const messageLink = (id: string) => `${MESSAGE_LINK_PREFIX}${id}`;
export const pageLink = (id: string) => `${PAGE_LINK_PREFIX}${id}`;
