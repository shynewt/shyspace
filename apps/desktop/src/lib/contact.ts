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

/**
 * A website as it reads in a row: its host and path, like
 * "ada.example.com/about". The host comes from the parsed address, so it is
 * the site that really opens. Null when it isn't a plain web address.
 */
export function shownWebsite(url: string): string | null {
  if (!isWebAddress(url) || hasUserInfo(url)) return null;
  try {
    const parsed = new URL(url);
    return `${parsed.host}${parsed.pathname}`.replace(/\/$/, "");
  } catch {
    return null;
  }
}

/** Whether a web address names a user before its host ("https://bank.example@evil.example"), which disguises where it goes. */
export function hasUserInfo(url: string): boolean {
  const authority = url.replace(WEB_ADDRESS, "").split(/[/?#]/)[0];
  return /[@\\]/.test(authority);
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
