import type { InvokeArgs } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import type { AlertAction, AlertTarget, LinkedMessage, Member, NotificationPermission } from "../../../src/lib/api";
import { argument, requireString } from "./helpers";

export const NOT_NOTIFICATIONS = Symbol("not a notification command");

type NotificationsContext = {
  /** Answers another command, as the core would for the same workspace. */
  answer: (command: string, args: InvokeArgs) => unknown;
  me: () => string;
  members: () => Member[];
  draftOf: (target: string) => string;
  setDraft: (target: string, text: string) => void;
  showAlert: (title: string, body: string, target: AlertTarget) => void;
};

/**
 * The system's notification permission and the quick actions on alerts
 * (macOS), as `notification_permission.rs` and `alert_actions.rs` answer.
 */
export class FakeNotifications {
  /** What the system says; anything but macOS always says "allowed". */
  permission: NotificationPermission = "allowed";
  /** What the person picks when the system asks. */
  answerWhenAsked: NotificationPermission = "allowed";
  permissionRequests = 0;
  settingsOpened = 0;
  /** Makes every reply from a notification fail to send. */
  failReplies = false;
  private pending: AlertAction[] = [];

  constructor(private readonly context: NotificationsContext) {}

  /** Someone used a button on an alert, as Apple's notification center reports it. */
  respond(action: AlertAction): void {
    this.pending.push(action);
    void emit("alert-action", null);
  }

  answer(command: string, args: InvokeArgs | undefined): unknown {
    switch (command) {
      case "notification_permission":
        return this.permission;
      case "request_notification_permission":
        this.permissionRequests += 1;
        if (this.permission === "not_asked") this.permission = this.answerWhenAsked;
        return this.permission;
      case "open_notification_settings":
        this.settingsOpened += 1;
        return null;
      case "take_alert_actions":
        return this.pending.splice(0);
      case "people_in":
        return { me: this.context.me(), members: this.context.members() };
      case "reply_to_alert":
        return this.reply(requireString(args, "message"), requireString(args, "text"));
      case "keep_unsent_reply":
        return this.keepUnsent(requireString(args, "message"), requireString(args, "text"));
      case "mark_alert_read":
        return this.markRead(argument(args, "target") as AlertTarget);
      default:
        return NOT_NOTIFICATIONS;
    }
  }

  private linked(message: string): LinkedMessage {
    const linked = this.context.answer("linked_message", { id: message }) as LinkedMessage | null;
    if (!linked) throw new Error("the message is no longer there");
    return linked;
  }

  private reply(message: string, text: string): unknown {
    if (this.failReplies) throw new Error("couldn't reach the workspace's storage");
    const { message: alerted, thread_root } = this.linked(message);
    this.context.answer("send_message", { channel: alerted.channel, text, replyTo: thread_root });
    return null;
  }

  private keepUnsent(message: string, text: string): null {
    const { message: alerted, thread_root } = this.linked(message);
    const target = thread_root ?? alerted.channel;
    const already = this.context.draftOf(target);
    this.context.setDraft(target, already.trim() ? `${already}\n\n${text}` : text);
    this.context.showAlert("Your reply wasn't sent", "It's saved as a draft.", { kind: "message", id: message });
    return null;
  }

  private markRead(target: AlertTarget): unknown {
    if (target.kind === "page") return this.context.answer("mark_page_mentions_read", { page: target.id });
    if (target.kind === "channel") throw new Error("a huddle has nothing to mark read");
    // As in the core: up to the alerted message, never past it.
    const { message: alerted, thread_root } = this.linked(target.id);
    if (thread_root) {
      return this.context.answer("mark_thread_read", { root: thread_root, message: alerted.id });
    }
    return this.context.answer("mark_read", { channel: alerted.channel, message: alerted.id });
  }
}
