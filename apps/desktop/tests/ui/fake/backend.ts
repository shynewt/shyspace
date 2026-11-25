/**
 * An in-memory stand-in for the Rust side of the desktop app, answering the
 * same `invoke` commands and emitting the same `change` events. It mirrors
 * the behavior the UI relies on (ordering, paging, which events fire), not
 * the crypto or sync underneath; those are covered by the Rust tests.
 */

import type { InvokeArgs } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import * as Y from "yjs";
import type {
  AlertTarget,
  AppIcon,
  BucketConfig,
  Change,
  Channel,
  Inbox,
  LaterItem,
  Member,
  MemberStatus,
  MessageCursor,
  OldKeyCheck,
  Page,
  PageEditorSession,
  ProfileDetails,
  ProtectionCheck,
  SectionSort,
  Session,
  StorageProtection,
  StorageProvider,
  StorageRotation,
  StorageSettings,
  WorkspaceBadge,
} from "../../../src/lib/api";
import { FakeChat, NOT_CHAT } from "./chat";
import { FakeDrafts } from "./drafts";
import { FakeFiles, NOT_FILES } from "./files";
import {
  argument,
  fakeSafetyCode,
  type Kind,
  noDetails,
  ownedId,
  randomHex,
  requireDetails,
  requireNotifyLevel,
  requireString,
  workspaceIdFor,
} from "./helpers";
import { FakeHuddles, NOT_HUDDLES } from "./huddles";
import { FakeInvites } from "./invites";
import { FakeLater } from "./later";
import { FakeNotifications, NOT_NOTIFICATIONS } from "./notifications";
import { FakePageCursors, NOT_PAGE_CURSORS } from "./pageCursors";
import { FakePageMentions, NOT_PAGE_MENTIONS } from "./pageMentions";
import { FakePrivate, NOT_PRIVATE } from "./private";
import { search } from "./search";
import { FakeSections } from "./sections";
import { sentMessages } from "./sent";
import type { LinkNames } from "./visibleText";
import { FakeWorkspaces, NOT_WORKSPACES } from "./workspaces";

/** The fake bucket refuses this secret, like a provider rejecting bad keys. */
export const REJECTED_SECRET = "wrong-secret";
/** What restore reads at most, as in the core. */
const RECOVERY_SNAPSHOT_LIMIT = 32 * 1024 * 1024;
/** A small workspace's snapshot, about what the core measures. */
const FAKE_SNAPSHOT_BYTES = 620_000;

type StoredPage = Omit<Page, "can_delete" | "is_saved"> & { doc: Y.Doc };

export class FakeBackend {
  session: Session | null = null;
  private isHoldingJoins = false;
  /** What the window title, taskbar and tray last showed as needing you. */
  attentionCount = 0;
  /** How the app shows itself outside its window; tests set `platform` to see another system's settings. */
  appIcon: AppIcon = { platform: "linux", is_in_menu_bar: false, shows_unread_count: true, is_tray_monochrome: false };
  private pendingJoin: { approve: () => void; cancel: () => void } | null = null;
  /** The update the app has downloaded and could restart into, as `check_for_update` reports it. */
  readyUpdate: string | null = null;
  /** How many times the app asked for an update. */
  updateChecks = 0;
  /** The version the app restarted into, once `restart_to_update` went through. */
  restartedInto: string | null = null;
  /** Makes `restart_to_update` fail with this, as a declined admin password or a blocked installer would. */
  restartFails: string | null = null;
  /** A huddle runs in another workspace on this device, so the core refuses to restart. */
  huddleElsewhere = false;
  /** Keeps `restart_to_update` waiting, as the system's password prompt does, until `finishRestart()`. */
  holdRestart = false;
  private finishHeldRestart: (() => void) | null = null;
  /** Keeps `drafts` loads waiting, as a busy disk would, until `finishDrafts()`. */
  holdDrafts = false;
  private heldDraftLoads: (() => void)[] = [];
  private members: Member[] = [];
  private pages: StoredPage[] = [];
  /** Each page editor session's page and Yjs client id, by its nonce. */
  private editorSessions = new Map<string, { page: string; client: number }>();
  /** The client id of the editor session started last. */
  latestEditorClient: number | null = null;
  private chat = new FakeChat({
    me: () => this.me(),
    workspaceId: () => this.session?.workspace_id ?? "",
    isAdmin: () => Boolean(this.session?.is_admin),
    members: () => this.members,
    tick: () => this.tick(),
    notify: (change) => this.notify(change),
    savedMark: (item) => this.later.mark(item),
    privateView: (channel) => this.private.view(channel),
    filesOf: (message) => this.files.filesOf(message),
    onMessageDeleted: (message) => this.files.dropFilesOf(message),
    pageTitle: (id) => this.pages.find((page) => page.id === id)?.title,
  });
  private private = new FakePrivate({
    me: () => this.me(),
    workspaceId: () => this.session?.workspace_id ?? "",
    isAdmin: () => Boolean(this.session?.is_admin),
    members: () => this.members,
    tick: () => this.tick(),
    notify: (change) => this.notify(change),
    savedMark: (item) => this.later.mark(item),
    privateView: (channel) => this.private.view(channel),
    filesOf: (message) => this.files.filesOf(message),
    onMessageDeleted: (message) => this.files.dropFilesOf(message),
    pageTitle: (id) => this.pages.find((page) => page.id === id)?.title,
    listChannel: (id, name) => this.chat.listChannel(id, name),
  });
  readonly files = new FakeFiles({
    me: () => this.me(),
    isAdmin: () => Boolean(this.session?.is_admin),
    members: () => this.members,
    notify: (change) => this.notify(change),
    sendMessage: (channel, text, replyTo) => this.answer("send_message", { channel, text, replyTo }) as string,
    channelOf: (message) => this.chat.channelOf(message),
    isPage: (id) => this.pages.some((page) => page.id === id),
  });
  readonly pageCursors = new FakePageCursors({
    notify: (change) => this.notify(change),
    docOf: (page) => {
      const found = this.pages.find((candidate) => candidate.id === page);
      if (!found) throw new Error(`fake backend: no page ${page}`);
      return found.doc;
    },
  });
  readonly pageMentions = new FakePageMentions({
    notify: (change) => this.notify(change),
    pageTitled: (title) => this.pageTitled(title),
    pageExists: (page) => this.pages.some((candidate) => candidate.id === page),
    me: () => this.me(),
    myName: () => this.myMember().name,
    workspaceId: () => this.session?.workspace_id ?? "",
    nameOf: (member) => this.members.find((candidate) => candidate.id === member)?.name ?? "",
  });
  readonly huddles = new FakeHuddles({
    me: () => this.me(),
    workspaceId: () => this.session?.workspace_id ?? "",
    channelName: (channel) => this.channelName(channel),
    members: () => this.members,
    notify: (change) => this.notify(change),
    privateView: (channel) => this.private.view(channel),
  });
  readonly workspaces = new FakeWorkspaces({
    session: () => this.session,
    setSession: (session) => {
      this.session = session;
    },
    emitFrom: (workspaceId, change) => void emit("change", { workspace_id: workspaceId, change }),
    shownBadge: () => this.shownBadge(),
  });
  private later = new FakeLater();
  private sections = new FakeSections();
  private drafts = new FakeDrafts();
  readonly notifications = new FakeNotifications({
    answer: (command, args) => this.answer(command, args),
    me: () => this.me(),
    members: () => this.members,
    draftOf: (target) => this.drafts.get(target),
    setDraft: (target, text) => {
      this.drafts.set(target, text);
      this.notify({ kind: "drafts" });
    },
    showAlert: (title, body, target) => {
      this.alertWorkspaces.push(this.session?.workspace_id ?? "");
      this.alertsShown.push({ title, body });
      this.alertPictures.push(null);
      this.alertPngs.push([]);
      this.alertTargets.push(target);
    },
  });
  private invites = new FakeInvites(() => this.notify({ kind: "invites" }));
  private storage: StorageSettings | null = null;
  private lamport = 0;

  /** Starts as the admin of a workspace with #general, as `create` would, who has already saved a recovery kit. */
  static withWorkspace(workspaceName: string, myName: string): FakeBackend {
    const backend = new FakeBackend();
    backend.createWorkspace(workspaceName, myName, {
      endpoint: "https://s3.example.com",
      region: "auto",
      bucket: "team",
      access_key_id: "AKIDEXAMPLE",
      secret_access_key: "secret",
    });
    backend.newRecoveryKit();
    return backend;
  }

  /** Starts as a regular member who joined Ada's workspace "Team". */
  static asMember(myName: string): FakeBackend {
    const backend = new FakeBackend();
    backend.joinWorkspace(myName);
    return backend;
  }

  // ---- Acting as other members, for tests --------------------------------

  addMember(name: string, { kind }: Kind = { kind: "person" }): string {
    const id = randomHex(32);
    this.members.push({
      id,
      name,
      is_bot: kind === "bot",
      is_admin: false,
      active: true,
      status: null,
      presence: "unreachable",
      details: noDetails(),
      picture: null,
      safety_code: fakeSafetyCode(id),
    });
    this.notify({ kind: "members" });
    return id;
  }

  memberNamed(name: string): string {
    const member = this.members.find((candidate) => candidate.name === name);
    if (!member) throw new Error(`fake backend: no member named ${name}`);
    return member.id;
  }

  /** What the app told the core about this device's presence, in order. */
  readonly ownPresenceLog: string[] = [];
  private isAppearingOffline = false;

  private storageProtection: ProtectionCheck = { state: "unknown", checked_ms: null };
  /** The bucket key change a removal starts, as the core keeps it on the admin's device. */
  private rotation: StorageRotation | null = null;
  /** The provider a removal's key change names; the core tells it from the endpoint. */
  storageProvider: StorageProvider = "r2";
  /** What `check_old_key_deleted` finds next. */
  oldKeyCheck: OldKeyCheck = "dead";

  private linkNames(): LinkNames {
    return {
      member: (id) => this.members.find((member) => member.id === id)?.name,
      page: (id) => this.pages.find((page) => page.id === id)?.title,
    };
  }

  /** Makes joins wait for `approveJoin`, as a real admin's answer does. */
  holdJoins(): void {
    this.isHoldingJoins = true;
  }

  approveJoin(): void {
    this.pendingJoin?.approve();
  }

  /** Answers the `drafts` loads held by `holdDrafts`, with the drafts as they are now. */
  finishDrafts(): void {
    if (this.heldDraftLoads.length === 0) throw new Error("fake backend: no drafts load is waiting");
    for (const answer of this.heldDraftLoads.splice(0)) answer();
  }

  /** Lets a restart held by `holdRestart` go on. */
  finishRestart(): void {
    if (!this.finishHeldRestart) throw new Error("fake backend: no restart is waiting");
    this.finishHeldRestart();
    this.finishHeldRestart = null;
  }

  private restartToUpdate(): string {
    if (this.huddleElsewhere || this.huddles.answer("my_huddle", undefined) !== null) return "in_huddle";
    if (this.readyUpdate === null) return "nothing_ready";
    if (this.restartFails !== null) throw new Error(this.restartFails);
    this.restartedInto = this.readyUpdate;
    return "restarting";
  }

  /** Sets what checking the bucket finds, as a provider's settings would. */
  setStorageProtection(state: StorageProtection): void {
    this.storageProtection = { state, checked_ms: Date.now() };
    this.notify({ kind: "storage_protection" });
  }

  /** Sets another member's presence, as their heartbeats would. */
  setPresence(member: string, presence: "online" | "away" | "unreachable"): void {
    const found = this.members.find((candidate) => candidate.id === member);
    if (!found) throw new Error(`fake backend: no member ${member}`);
    found.presence = presence;
    this.notify({ kind: "members" });
  }

  /** Sets another member's status, as their own device would. */
  setMemberStatus(member: string, status: MemberStatus | null): void {
    const found = this.members.find((candidate) => candidate.id === member);
    if (!found) throw new Error(`fake backend: no member ${member}`);
    found.status = status;
    this.notify({ kind: "members" });
  }

  /** Sets what another member shares about themselves, as their own device would. */
  setMemberDetails(member: string, details: Partial<ProfileDetails>): void {
    const found = this.members.find((candidate) => candidate.id === member);
    if (!found) throw new Error(`fake backend: no member ${member}`);
    found.details = { ...noDetails(), ...details };
    this.notify({ kind: "members" });
  }

  /** Desktop notifications the app asked to show. */
  readonly alertsShown: { title: string; body: string }[] = [];
  /** The bodies of alerts a muted workspace or the pause held back. */
  readonly alertsHeld: string[] = [];
  /** Whose picture each alert in `alertsShown` came with, if it was a PNG; null for none. */
  readonly alertPictures: (string | null)[] = [];
  /** The PNG bytes each alert in `alertsShown` came with; empty for none. */
  readonly alertPngs: number[][] = [];
  /** What each alert in `alertsShown` opens when clicked, if anything. */
  readonly alertTargets: (AlertTarget | null)[] = [];
  /** Which workspace each alert in `alertsShown` came from. */
  readonly alertWorkspaces: string[] = [];

  /** `author` writes in `channelName` of another workspace on this device, raising an alert there. */
  alertFrom(workspaceId: string, author: string, channelName: string, preview: string): void {
    const change: Change = {
      kind: "alert",
      channel: `${workspaceId}-${channelName}`,
      channel_name: channelName,
      message: `${workspaceId}-message`,
      author: `${workspaceId}-${author}`,
      author_name: author,
      preview,
    };
    void emit("change", { workspace_id: workspaceId, change });
  }

  /** Clicks the `index`th alert, as the notification server reports a click. */
  clickAlert(index: number): void {
    const target = this.alertTargets[index];
    if (!target) throw new Error(`fake backend: alert ${index} opens nothing`);
    this.notifications.respond({ action: "open", workspace_id: this.alertWorkspaces[index] ?? null, target });
  }

  /** Someone else made a private channel with you (and `others`) in it. */
  receivePrivateChannel(owner: string, name: string, others: string[] = []): string {
    return this.private.receive(owner, name, others);
  }

  /** The owner of the private channel `name` removed you from it. */
  removeMeFromPrivateChannel(name: string): void {
    this.private.receiveChange(this.chat.channelNamed(name).id, { removed: this.me() });
  }

  /** `member` opened the private channel `name`, so their device wrote that they joined. */
  privateMemberJoined(name: string, member: string): void {
    this.private.receiveChange(this.chat.channelNamed(name).id, { joined: member });
  }

  /** The owner of the private channel `name` started a new key your device didn't get. */
  missNewKeyIn(name: string): void {
    this.private.receiveChange(this.chat.channelNamed(name).id, { isMissingKey: true });
  }

  /** The owner of the private channel `name` made you its owner. */
  makeMeOwnerOf(name: string): void {
    this.private.receiveChange(this.chat.channelNamed(name).id, { owner: this.me() });
  }

  /** `member` was removed from the workspace. */
  removeFromWorkspace(member: string): void {
    const found = this.members.find((candidate) => candidate.id === member);
    if (!found) throw new Error(`fake backend: no member ${member}`);
    found.active = false;
    this.notify({ kind: "members" });
  }

  receiveMessage(author: string, channelName: string, text: string): string {
    return this.chat.receiveMessage(author, channelName, text);
  }

  receiveMessageIn(author: string, channelId: string, text: string): string {
    return this.chat.receiveMessageIn(author, channelId, text);
  }

  receiveReply(author: string, root: string, text: string): string {
    return this.chat.receiveReply(author, root, text);
  }

  receiveReaction(author: string, message: string, emoji: string): void {
    this.chat.receiveReaction(author, message, emoji);
  }

  channelId(name: string): string {
    return this.chat.channelNamed(name).id;
  }

  messageWithText(text: string): string {
    return this.chat.messageWithText(text);
  }

  receiveJoinRequest(name: string, kind: Kind = { kind: "person" }): string {
    return this.invites.receiveRequest(name, kind);
  }

  /** Rebuilds a page from scratch with `text`, as the core does after undoing a removed member's edits. */
  rebuildPage(title: string, text: string): void {
    const page = this.pageTitled(title);
    page.doc = new Y.Doc();
    page.doc.getText("content").insert(0, text);
    this.notify({ kind: "page_reset", page: page.id });
  }

  pageText(title: string): string {
    return this.pageTitled(title).doc.getText("content").toString();
  }

  /** Adds a page of yours that already holds `text`; `parentTitle` nests it. */
  seedPage(title: string, text: string, parentTitle?: string): string {
    const doc = new Y.Doc();
    doc.getText("content").insert(0, text);
    const parent = parentTitle === undefined ? null : this.pageTitled(parentTitle).id;
    const id = ownedId(this.me());
    this.pages.push({ id, parent, title, updated_ms: Date.now(), doc });
    this.tick();
    this.notify({ kind: "pages" });
    return id;
  }

  /**
   * Another member's edit to a page arriving: made on their own copy, which
   * may lack your latest keystrokes, then sent as one Yjs update.
   */
  receivePageEdit(title: string, edit: { at: number; remove?: number; insert?: string }): void {
    const page = this.pageTitled(title);
    const theirs = new Y.Doc();
    Y.applyUpdate(theirs, Y.encodeStateAsUpdate(page.doc));
    const before = Y.encodeStateVector(theirs);
    const text = theirs.getText("content");
    if (edit.remove) text.delete(edit.at, edit.remove);
    if (edit.insert) text.insert(edit.at, edit.insert);
    const update = Y.encodeStateAsUpdate(theirs, before);
    Y.applyUpdate(page.doc, update);
    this.notify({ kind: "doc", page: page.id, update: Array.from(update) });
  }

  /** `author` adding a paragraph to a page that mentions you: `before`, the mention, then `after`. */
  receivePageMention(title: string, author: string, before: string, after = "", isAlerting = true): void {
    this.pageMentions.receive(title, author, before, after, isAlerting);
  }

  /** Another member's edit written under your editor's client id, as a forged update would be. */
  receiveEditUnderMyEditorsId(title: string, insert: string): void {
    const page = this.pageTitled(title);
    if (this.latestEditorClient === null) throw new Error("fake backend: no editor session yet");
    const forged = new Y.Doc();
    Y.applyUpdate(forged, Y.encodeStateAsUpdate(page.doc));
    // Set after loading: Yjs drops a client id it finds in an update it receives.
    forged.clientID = this.latestEditorClient;
    const before = Y.encodeStateVector(forged);
    const text = forged.getText("content");
    text.insert(text.length, insert);
    const update = Y.encodeStateAsUpdate(forged, before);
    Y.applyUpdate(page.doc, update);
    this.notify({ kind: "doc", page: page.id, update: Array.from(update) });
  }

  private pageTitled(title: string): StoredPage {
    const page = this.pages.find((candidate) => candidate.title === title);
    if (!page) throw new Error(`fake backend: no page titled "${title}"`);
    return page;
  }

  // ---- The command surface ----------------------------------------------

  /** Answers a command with a copy, as crossing the real IPC boundary would; a later answer is copied when it comes. */
  handle(command: string, args: InvokeArgs | undefined): unknown {
    const answer = this.answer(command, args);
    return answer instanceof Promise ? answer.then((value) => structuredClone(value)) : structuredClone(answer);
  }

  private answer(command: string, args: InvokeArgs | undefined): unknown {
    const filesAnswer = this.files.answer(command, args);
    if (filesAnswer !== NOT_FILES) return filesAnswer;
    const cursorsAnswer = this.pageCursors.answer(command, args);
    if (cursorsAnswer !== NOT_PAGE_CURSORS) return cursorsAnswer;
    const huddlesAnswer = this.huddles.answer(command, args);
    if (huddlesAnswer !== NOT_HUDDLES) return huddlesAnswer;
    const workspacesAnswer = this.workspaces.answer(command, args);
    if (workspacesAnswer !== NOT_WORKSPACES) return workspacesAnswer;
    const privateAnswer = this.private.answer(command, args);
    if (privateAnswer !== NOT_PRIVATE) return privateAnswer;
    const notificationsAnswer = this.notifications.answer(command, args);
    if (notificationsAnswer !== NOT_NOTIFICATIONS) return notificationsAnswer;
    const mentionsAnswer = this.pageMentions.answer(command, args);
    if (mentionsAnswer !== NOT_PAGE_MENTIONS) return mentionsAnswer;
    if (command === "mark_all_read") this.pageMentions.markAllRead();
    const chatAnswer = this.chat.answer(command, args);
    if (command === "inbox") return { ...(chatAnswer as Inbox), page_mentions: this.pageMentions.unread() };
    if (command === "send_message" && typeof chatAnswer === "string") {
      const target = this.chat.draftTargetOf(chatAnswer);
      if (target) this.drafts.set(target, "");
      this.notify({ kind: "drafts" });
    }
    if (chatAnswer !== NOT_CHAT) return chatAnswer;
    switch (command) {
      case "plugin:app|version":
        return "1.2.3";
      case "check_for_update":
        this.updateChecks += 1;
        return this.readyUpdate;
      case "restart_to_update":
        if (!this.holdRestart) return this.restartToUpdate();
        return new Promise((resolve, reject) => {
          this.finishHeldRestart = () => {
            try {
              resolve(this.restartToUpdate());
            } catch (error) {
              reject(error);
            }
          };
        });
      case "sidebar_sections":
        return this.sections.list(
          this.chat.channels.map((channel) => ({
            id: channel.id,
            name: channel.name,
            lastActivity: Math.max(
              0,
              ...this.chat.messages
                .filter((message) => message.channel === channel.id)
                .map((message) => message.created_ms),
            ),
          })),
        );
      case "create_section": {
        const id = this.sections.create(requireString(args, "name"));
        this.notify({ kind: "channels" });
        return id;
      }
      case "rename_section":
        this.sections.rename(requireString(args, "section"), requireString(args, "name"));
        return this.notify({ kind: "channels" });
      case "delete_section":
        this.sections.delete(requireString(args, "section"));
        return this.notify({ kind: "channels" });
      case "set_section_sort":
        this.sections.setSort(requireString(args, "section"), requireString(args, "sort") as SectionSort);
        return this.notify({ kind: "channels" });
      case "set_section_collapsed":
        this.sections.setCollapsed(requireString(args, "section"), argument(args, "collapsed") === true);
        return this.notify({ kind: "channels" });
      case "move_channel":
        this.sections.move(requireString(args, "channel"), (argument(args, "section") as string | null) ?? null);
        return this.notify({ kind: "channels" });
      case "set_status": {
        const me = this.myMember();
        me.status = {
          emoji: requireString(args, "emoji"),
          text: requireString(args, "text"),
          until_ms: (argument(args, "untilMs") as number | null) ?? null,
          alerts: requireNotifyLevel(args, "alerts"),
        };
        return this.notify({ kind: "members" });
      }
      case "clear_status":
        this.myMember().status = null;
        return this.notify({ kind: "members" });
      case "set_away":
        this.ownPresenceLog.push(argument(args, "away") === true ? "away" : "back");
        return this.refreshOwnPresence();
      case "appear_offline":
        return this.isAppearingOffline;
      case "set_appear_offline":
        this.isAppearingOffline = argument(args, "offline") === true;
        this.ownPresenceLog.push(this.isAppearingOffline ? "offline" : "visible");
        return this.refreshOwnPresence();
      case "set_profile": {
        const me = this.members.find((member) => member.id === this.me());
        if (!me) throw new Error("fake backend: no member for me");
        me.name = requireString(args, "name").trim();
        return this.notify({ kind: "members" });
      }
      case "set_profile_details":
        this.myMember().details = requireDetails(args);
        return this.notify({ kind: "members" });
      case "save_for_later":
        this.later.save(requireString(args, "item"), (argument(args, "remindMs") as number | null) ?? null);
        return this.notifyLater();
      case "remove_from_later":
        this.later.remove(requireString(args, "item"));
        return this.notifyLater();
      case "mark_later_done":
        this.later.setDone(requireString(args, "item"), argument(args, "done") === true);
        return this.notifyLater();
      case "later":
        return this.laterItems();
      case "take_due_reminders": {
        const due = this.later.takeDue(Date.now());
        if (due.length > 0) this.notify({ kind: "later" });
        // Every workspace shares the fake's one list, so each is the shown workspace's.
        const workspaceId = this.session?.workspace_id ?? "";
        return this.laterItems()
          .filter((item) => due.includes(item.id))
          .map((item) => ({ workspace_id: workspaceId, item }));
      }
      case "next_reminder_ms": {
        const now = Date.now();
        const upcoming = this.laterItems().flatMap((item) =>
          !item.is_done && item.remind_ms !== null && item.remind_ms > now ? [item.remind_ms] : [],
        );
        return upcoming.length > 0 ? Math.min(...upcoming) : null;
      }
      case "set_draft":
        this.drafts.set(requireString(args, "target"), requireString(args, "text"));
        return this.notify({ kind: "drafts" });
      case "drafts": {
        const list = () => this.drafts.list((target) => this.chat.channelOf(target));
        if (!this.holdDrafts) return list();
        return new Promise((resolve) => this.heldDraftLoads.push(() => resolve(list())));
      }
      case "sent_messages":
        return sentMessages(this.chat, {
          me: this.me(),
          isReadable: (channel) => this.private.mayPost(channel),
          text: requireString(args, "text"),
          before: (argument(args, "before") as MessageCursor | null) ?? null,
          limit: argument(args, "limit") as number,
        });
      case "session":
        return this.session;
      case "create_workspace":
        return this.createWorkspace(
          requireString(args, "workspaceName"),
          requireString(args, "name"),
          argument(args, "bucket") as BucketConfig,
        );
      case "join_workspace":
        if (requireString(args, "invite").startsWith("shy2_")) {
          throw new Error("This invite was made by an older version of shyspace. Ask for a new one.");
        }
        return this.requestToJoin(requireString(args, "name"));
      case "cancel_join":
        this.pendingJoin?.cancel();
        return null;
      case "set_attention_count":
        this.attentionCount = argument(args, "count") as number;
        return null;
      case "app_icon":
        return { ...this.appIcon };
      case "set_in_menu_bar":
        this.appIcon.is_in_menu_bar = argument(args, "isShown") === true;
        return null;
      case "set_shows_unread_count":
        this.appIcon.shows_unread_count = argument(args, "isShown") === true;
        return null;
      case "set_tray_monochrome":
        this.appIcon.is_tray_monochrome = argument(args, "isMonochrome") === true;
        return null;
      case "test_bucket":
        return this.testBucket(argument(args, "bucket") as BucketConfig);
      case "safety_code":
        return "8506 3956 2869 2874 2976";
      case "recovery_kit":
        return this.newRecoveryKit();
      case "storage_settings":
        return this.requireStorage();
      case "storage_protection":
        return this.storageProtection;
      case "check_storage_protection":
        this.storageProtection = { ...this.storageProtection, checked_ms: Date.now() };
        this.notify({ kind: "storage_protection" });
        return this.storageProtection;
      case "storage_rotation":
        if (!this.session?.is_admin) throw new Error("only the admin can see the storage key change");
        return this.rotation;
      case "check_old_key_deleted":
        return this.checkOldKeyDeleted();
      case "cancel_storage_rotation":
        this.rotation = null;
        this.notify({ kind: "key_rotation" });
        return null;
      case "update_storage":
        return this.updateStorage(requireString(args, "region"), requireString(args, "accessKeyId"));
      case "show_alert":
        // As in the core: a muted workspace, or the pause, holds it back.
        if (this.workspaces.isSilenced(requireString(args, "workspaceId"))) {
          this.alertsHeld.push(requireString(args, "body"));
          return null;
        }
        this.alertWorkspaces.push(requireString(args, "workspaceId"));
        this.alertsShown.push({ title: requireString(args, "title"), body: requireString(args, "body") });
        this.alertPictures.push(alertPicture(argument(args, "picture")));
        this.alertPngs.push(alertPng(argument(args, "picture")));
        this.alertTargets.push(alertTarget(argument(args, "target")));
        return null;
      case "pages":
        return this.pages.map(({ doc: _doc, ...page }) => ({
          ...page,
          parent: this.pages.some((candidate) => candidate.id === page.parent) ? page.parent : null,
          can_delete: this.mayDelete(page.id),
          is_saved: this.later.has(page.id),
        }));
      case "create_page":
        return this.createPage(requireString(args, "title"), (argument(args, "parent") as string | null) ?? null);
      case "delete_page":
        return this.deletePage(requireString(args, "page"));
      case "rename_page":
        return this.renamePage(requireString(args, "page"), requireString(args, "title"));
      case "page_state":
        return Y.encodeStateAsUpdate(this.page(requireString(args, "page")).doc);
      case "page_editor_session":
        return this.pageEditorSession(requireString(args, "page"));
      case "apply_doc_update":
        return this.applyDocUpdate(
          requireString(args, "page"),
          requireString(args, "nonce"),
          argument(args, "update") as number[],
        );
      case "members":
        return this.members;
      case "remove_member":
        return this.removeMember(requireString(args, "member"));
      case "invites":
        return this.invites.list();
      case "create_invite":
        return `shy3_${this.invites.create(requireString(args, "label")).token}`;
      case "approve_invite":
        return this.approveInvite(requireString(args, "token"));
      case "reject_invite":
        return this.invites.setState(requireString(args, "token"), "rejected");
      case "revoke_invite":
        return this.invites.setState(requireString(args, "token"), "revoked");
      case "search":
        return search(requireString(args, "query"), {
          messages: this.chat.messages,
          channels: this.chat.channels,
          pages: this.pages,
          names: this.linkNames(),
        });
      case "status":
        return {
          peers: 1,
          last_sync_ms: Date.now(),
          error: null,
          removed: false,
          needs_update: false,
          needs_update_from: [],
        };
      default:
        throw new Error(`fake backend: unknown command "${command}"`);
    }
  }

  // ---- Workspace --------------------------------------------------------

  private createWorkspace(workspaceName: string, name: string, bucket: BucketConfig): Session {
    const me = randomHex(32);
    this.members = [
      {
        id: me,
        name,
        is_bot: false,
        is_admin: true,
        active: true,
        status: null,
        presence: "online",
        details: noDetails(),
        picture: null,
        safety_code: fakeSafetyCode(me),
      },
    ];
    this.storage = {
      workspace_id: "a1b2c3d4e5f60718",
      endpoint: bucket.endpoint,
      bucket: bucket.bucket,
      region: bucket.region,
      access_key_id: bucket.access_key_id,
      recovery_snapshot_bytes: null,
      recovery_snapshot_limit: RECOVERY_SNAPSHOT_LIMIT,
    };
    this.session = { workspace_id: workspaceIdFor("Acme"), workspace_name: workspaceName, me, is_admin: true };
    this.chat.createChannel("general");
    return this.session;
  }

  private requestToJoin(name: string): Session | Promise<Session> {
    if (!this.isHoldingJoins) return this.joinWorkspace(name);
    return new Promise((resolve, reject) => {
      this.pendingJoin = {
        approve: () => resolve(this.joinWorkspace(name)),
        cancel: () => reject("you cancelled the join request"),
      };
    });
  }

  private joinWorkspace(name: string): Session {
    const admin = randomHex(32);
    const me = randomHex(32);
    this.members = [
      {
        id: admin,
        name: "Ada",
        is_bot: false,
        is_admin: true,
        active: true,
        status: null,
        presence: "unreachable",
        details: noDetails(),
        picture: null,
        safety_code: fakeSafetyCode(admin),
      },
      {
        id: me,
        name,
        is_bot: false,
        is_admin: false,
        active: true,
        status: null,
        presence: "online",
        details: noDetails(),
        picture: null,
        safety_code: fakeSafetyCode(me),
      },
    ];
    this.chat.addChannel(ownedId(admin), "general");
    // Members get the bucket settings with their welcome, as in the core.
    this.storage = {
      workspace_id: "a1b2c3d4e5f60718",
      endpoint: "https://s3.example.com",
      bucket: "team",
      region: "auto",
      access_key_id: "AKIDEXAMPLE",
      recovery_snapshot_bytes: null,
      recovery_snapshot_limit: RECOVERY_SNAPSHOT_LIMIT,
    };
    this.session = { workspace_id: workspaceIdFor("Team"), workspace_name: "Team", me, is_admin: false };
    return this.session;
  }

  private testBucket(bucket: BucketConfig): void {
    if (bucket.secret_access_key === REJECTED_SECRET) {
      throw "the bucket refused these keys: 403 Forbidden: The request signature we calculated does not match";
    }
  }

  private requireStorage(): StorageSettings {
    if (!this.storage) throw new Error("fake backend: no workspace");
    return this.storage;
  }

  /** As in the core, the new kit comes back only if the admin has made one before. */
  private updateStorage(region: string, accessKeyId: string): string | null {
    this.storage = { ...this.requireStorage(), region, access_key_id: accessKeyId };
    this.notify({ kind: "storage" });
    if (this.rotation) {
      this.rotation = { ...this.rotation, has_new_key: true };
      this.notify({ kind: "key_rotation" });
    }
    return this.requireStorage().recovery_snapshot_bytes === null ? null : this.newRecoveryKit();
  }

  /** Making a kit uploads a snapshot, as in the core. */
  private newRecoveryKit(): string {
    this.storage = { ...this.requireStorage(), recovery_snapshot_bytes: FAKE_SNAPSHOT_BYTES };
    return `shyrec1_${randomHex(48)}`;
  }

  // ---- Pages ------------------------------------------------------------

  private createPage(title: string, parent: string | null): string {
    const id = ownedId(this.me());
    this.pages.push({ id, parent, title, updated_ms: Date.now(), doc: new Y.Doc() });
    this.tick();
    this.notify({ kind: "pages" });
    return id;
  }

  private page(id: string): StoredPage {
    const page = this.pages.find((candidate) => candidate.id === id);
    if (!page) throw new Error(`fake backend: no page ${id}`);
    return page;
  }

  private renamePage(id: string, title: string): void {
    const page = this.page(id);
    page.title = title;
    page.updated_ms = Date.now();
    this.tick();
    this.notify({ kind: "pages" });
  }

  private pageEditorSession(page: string): PageEditorSession {
    const session = { client_id: Number.parseInt(randomHex(6), 16), nonce: randomHex(16) };
    this.editorSessions.set(session.nonce, { page, client: session.client_id });
    this.latestEditorClient = session.client_id;
    return session;
  }

  /** Like the core, takes pieces only for the session's own client id. */
  private applyDocUpdate(id: string, nonce: string, update: number[]): void {
    const session = this.editorSessions.get(nonce);
    // The core binds a client id to its page too, so another page's nonce gives another id.
    if (session?.page !== id) throw new Error("fake backend: no editor session for this page");
    const { client } = session;
    const bytes = Uint8Array.from(update);
    if (Y.decodeUpdate(bytes).structs.some((struct) => struct.id.client !== client)) {
      throw new Error("damaged page update: page update adds pieces for another editor's client id");
    }
    const page = this.page(id);
    Y.applyUpdate(page.doc, bytes);
    page.updated_ms = Date.now();
    this.tick();
    this.notify({ kind: "doc", page: id, update });
  }

  private deletePage(id: string): void {
    this.requireMayDelete(id);
    this.pages = this.pages.filter((page) => page.id !== id);
    this.files.dropFilesOfPage(id);
    this.notify({ kind: "pages" });
  }

  // ---- Members and invites ----------------------------------------------

  private removeMember(id: string): void {
    const member = this.members.find((candidate) => candidate.id === id);
    if (!member) throw new Error(`fake backend: no member ${id}`);
    member.active = false;
    this.notify({ kind: "members" });
    // As in the core: the removed member still holds the bucket key, so a new one is due.
    this.rotation = {
      provider: this.storageProvider,
      has_new_key: false,
      members: this.members
        .filter((candidate) => candidate.active && candidate.id !== this.me())
        .map((candidate) => ({ member: candidate.id, has_moved: false })),
      old_key: "not_checked",
    };
    this.notify({ kind: "key_rotation" });
  }

  /** `name`'s device starts using the new key, as its head in the bucket would say. */
  memberMovesToNewKey(name: string): void {
    const id = this.memberNamed(name);
    if (!this.rotation) throw new Error("fake backend: no key change under way");
    this.rotation = {
      ...this.rotation,
      members: this.rotation.members.map((move) => (move.member === id ? { ...move, has_moved: true } : move)),
    };
    this.notify({ kind: "key_rotation" });
  }

  /** As in the core: a dead old key ends the change only once the key in use is a new one. */
  private checkOldKeyDeleted(): OldKeyCheck {
    if (!this.rotation) throw new Error("fake backend: no key change under way");
    const found = this.oldKeyCheck;
    if (found !== "dead") this.rotation = { ...this.rotation, old_key: found };
    else if (this.rotation.has_new_key) this.rotation = null;
    this.notify({ kind: "key_rotation" });
    return found;
  }

  private approveInvite(token: string): void {
    this.members.push(this.invites.approve(token));
    this.notify({ kind: "members" });
  }

  // ---- Plumbing ---------------------------------------------------------

  private refreshOwnPresence(): void {
    const isAway = this.ownPresenceLog.findLast((entry) => entry === "away" || entry === "back") === "away";
    this.myMember().presence = this.isAppearingOffline ? "unreachable" : isAway ? "away" : "online";
    this.notify({ kind: "members" });
  }

  private myMember(): Member {
    const me = this.members.find((member) => member.id === this.me());
    if (!me) throw new Error("fake backend: no member for me");
    return me;
  }

  private me(): string {
    if (!this.session) throw new Error("fake backend: no workspace is open");
    return this.session.me;
  }

  private mayDelete(id: string): boolean {
    return Boolean(this.session?.is_admin) || id.startsWith(this.me().slice(0, 32));
  }

  private requireMayDelete(id: string): void {
    if (!this.mayDelete(id)) throw "only its owner or the admin can do that";
  }

  private tick(): number {
    this.lamport += 1;
    return this.lamport;
  }

  private laterItems(): LaterItem[] {
    const pages = this.answer("pages", undefined) as Page[];
    return this.later.list((item) => ({
      message: this.chat.linkedMessage(item),
      page: pages.find((page) => page.id === item) ?? null,
    }));
  }

  private notifyLater(): void {
    this.notify({ kind: "later" });
    for (const channel of this.chat.channels) this.notify({ kind: "messages", channel: channel.id });
    this.notify({ kind: "pages" });
  }

  /** As the core counts a workspace's badge: the Inbox outside DMs, mentions in pages, unread unmuted DMs, and due reminders. */
  private shownBadge(): Omit<WorkspaceBadge, "workspace_id"> {
    const channels = this.answer("channels", undefined) as Channel[];
    const inbox = this.answer("inbox", undefined) as Inbox;
    const dms = new Set(channels.filter((channel) => channel.kind === "dm").map((channel) => channel.id));
    const outsideDms = inbox.attention.filter((item) => !dms.has(item.message.channel)).length;
    const inPages = inbox.page_mentions.length;
    const inDms = channels
      .filter((channel) => channel.kind === "dm" && channel.notify !== "nothing")
      .reduce((total, channel) => total + channel.unread, 0);
    const now = Date.now();
    const due = this.laterItems().filter(
      (item) => !item.is_done && item.remind_ms !== null && item.remind_ms <= now,
    ).length;
    return {
      unread: inPages > 0 || channels.some((channel) => channel.unread > 0 && channel.notify !== "nothing"),
      mentions: inPages + inDms + channels.reduce((total, channel) => total + channel.unread_mentions, 0),
      attention: outsideDms + inPages + inDms + due,
    };
  }

  /** "#general", or a DM's people joined by ", ", as the core names a huddle's place. */
  private channelName(id: string): string {
    const channels = this.answer("channels", undefined) as { id: string; name: string; dm_members: string[] }[];
    const channel = channels.find((candidate) => candidate.id === id);
    if (!channel) return "";
    if (channel.dm_members.length === 0) return `#${channel.name}`;
    return channel.dm_members
      .map((member) => this.members.find((found) => found.id === member)?.name ?? member)
      .join(", ");
  }

  private notify(change: Change): void {
    void emit("change", { workspace_id: this.session?.workspace_id ?? null, change });
  }
}

/** A `show_alert` target, checked as the Tauri command's types would. */
function alertTarget(value: unknown): AlertTarget | null {
  if (value === null || value === undefined) return null;
  if (typeof value !== "object" || !("kind" in value) || !("id" in value) || typeof value.id !== "string") {
    throw new Error("fake backend: an alert target is { kind, id }");
  }
  if (value.kind === "page") {
    if (!("position" in value) || !Array.isArray(value.position)) {
      throw new Error("fake backend: a page alert target is { kind, id, position }");
    }
    return { kind: "page", id: value.id, position: value.position.filter((byte) => typeof byte === "number") };
  }
  if (value.kind !== "message" && value.kind !== "channel") throw new Error("fake backend: unknown alert target");
  return { kind: value.kind, id: value.id };
}

/** The member a `show_alert` picture is for, if it's PNG bytes. */
function alertPng(picture: unknown): number[] {
  if (typeof picture !== "object" || picture === null || !("png" in picture) || !Array.isArray(picture.png)) return [];
  return picture.png.filter((byte): byte is number => typeof byte === "number");
}

function alertPicture(picture: unknown): string | null {
  if (picture === null || picture === undefined) return null;
  if (typeof picture !== "object" || !("member" in picture) || !("png" in picture)) {
    throw new Error("fake backend: an alert picture is { member, png }");
  }
  const { member, png } = picture;
  const signature = [0x89, 0x50, 0x4e, 0x47];
  const isPng = Array.isArray(png) && signature.every((byte, index) => png[index] === byte);
  if (typeof member !== "string" || !isPng) throw new Error("fake backend: an alert picture must be PNG bytes");
  return member;
}
