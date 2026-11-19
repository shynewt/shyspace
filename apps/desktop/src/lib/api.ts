import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type Session = { workspace_name: string; me: string; is_admin: boolean };
export type Channel = { id: string; name: string };
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
  reactions: Reaction[];
};
export type Page = { id: string; parent: string | null; title: string; updated_ms: number };
export type Member = { id: string; name: string; is_bot: boolean; is_admin: boolean; active: boolean };
export type SearchHit = { kind: "message" | "page"; id: string; title: string; snippet: string; channel: string | null };
export type SyncStatus = { peers: number; last_sync_ms: number | null; error: string | null; removed: boolean };
export type BucketConfig = {
  endpoint: string;
  region: string;
  bucket: string;
  access_key_id: string;
  secret_access_key: string;
};

export type Change =
  | { kind: "channels" }
  | { kind: "messages"; channel: string }
  | { kind: "pages" }
  | { kind: "doc"; page: string; update: number[] }
  | { kind: "members" }
  | { kind: "sync" }
  | { kind: "removed" };

export const api = {
  session: () => invoke<Session | null>("session"),
  createWorkspace: (workspaceName: string, name: string, bucket: BucketConfig) =>
    invoke<Session>("create_workspace", { workspaceName, name, bucket }),
  joinWorkspace: (invite: string, name: string) => invoke<Session>("join_workspace", { invite, name }),
  channels: () => invoke<Channel[]>("channels"),
  createChannel: (name: string) => invoke<string>("create_channel", { name }),
  messages: (channel: string, limit = 200) => invoke<Message[]>("messages", { channel, limit }),
  sendMessage: (channel: string, text: string, replyTo: string | null) =>
    invoke<string>("send_message", { channel, text, replyTo }),
  react: (message: string, emoji: string, active: boolean) => invoke<void>("react", { message, emoji, active }),
  pages: () => invoke<Page[]>("pages"),
  createPage: (title: string, parent: string | null) => invoke<string>("create_page", { title, parent }),
  renamePage: (page: string, title: string) => invoke<void>("rename_page", { page, title }),
  pageState: async (page: string) => new Uint8Array(await invoke<ArrayBuffer>("page_state", { page })),
  applyDocUpdate: (page: string, update: Uint8Array) =>
    invoke<void>("apply_doc_update", { page, update: Array.from(update) }),
  members: () => invoke<Member[]>("members"),
  removeMember: (member: string) => invoke<void>("remove_member", { member }),
  inviteCode: () => invoke<string>("invite_code"),
  search: (query: string) => invoke<SearchHit[]>("search", { query }),
  status: () => invoke<SyncStatus>("status"),
};

type ChangeListener = (change: Change) => void;
const listeners = new Set<ChangeListener>();
let listening: Promise<unknown> | null = null;

/** One native subscription, fanned out to every component that cares. */
export function onChange(listener: ChangeListener): () => void {
  listening ??= listen<Change>("change", (event) => listeners.forEach((notify) => notify(event.payload)));
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export const PAGE_LINK_PREFIX = "shy://page/";
export const pageLink = (id: string) => `${PAGE_LINK_PREFIX}${id}`;
