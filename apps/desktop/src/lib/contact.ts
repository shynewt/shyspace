/**
 * Opening addresses with the system's own handlers: mail for an email
 * address, the phone app for a number, the browser for a website. Every
 * external link in the app opens through here.
 */
import { openUrl } from "@tauri-apps/plugin-opener";
import { reportError } from "./toast";

const WEB_ADDRESS = /^https?:\/\//i;

export function mailtoUrl(email: string): string {
  return `mailto:${email}`;
}

/** `tel:` with only the digits and a leading +, which every dialer reads. */
export function telUrl(phone: string): string {
  return `tel:${phone.replace(/[^0-9+]/g, "")}`;
}

export function isWebAddress(url: string): boolean {
  return WEB_ADDRESS.test(url);
}

/** A website as it reads in a row: "ada.example.com/about", without the scheme or a trailing slash. */
export function shownWebsite(url: string): string {
  return url.replace(WEB_ADDRESS, "").replace(/\/$/, "");
}

const SCHEME = /^[a-z][a-z0-9+.-]*:/i;

/**
 * `url` as it may be handed to the system: the scheme lowercased (the
 * window's allowed schemes match case by case), and a mail link without its
 * query, which could ask the mail app to attach files (`?attach=…`).
 */
export function openableUrl(url: string): string {
  const lowered = url.replace(SCHEME, (scheme) => scheme.toLowerCase());
  return lowered.startsWith("mailto:") ? lowered.split("?")[0] : lowered;
}

export function openExternalUrl(url: string): Promise<void> {
  return openUrl(openableUrl(url));
}

export function openExternal(url: string): void {
  openExternalUrl(url).catch(reportError);
}
