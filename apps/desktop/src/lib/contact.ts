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
  return /[@\\]/.test(authorityOf(url));
}

function authorityOf(url: string): string {
  return url.replace(WEB_ADDRESS, "").split(/[/?#]/)[0];
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

/** One word that reads as a web address, in any script: "paypal.com", "https://bank.example/login". */
const ADDRESS_WORD = /^(?:https?:\/\/)?(?:[\p{L}\p{N}-]+\.)+\p{L}{2,}(?:[:/?#]\S*)?$/iu;
const WORD_EDGE_PUNCTUATION = /^[("'«“]+|[.,;:!?)"'»”]+$/gu;
/** Characters that draw nothing (zero-width spaces and joiners, soft hyphens, direction marks): one can split an address so it no longer reads as one. */
const INVISIBLE = /\p{Cf}/gu;

/** A URL's host as the system opens it: look-alike letters in punycode, no "www.", and null if it isn't a URL. */
function hostOf(url: string): string | null {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    // Not an address at all; callers treat an unknown host as one to ask about.
    return null;
  }
}

/** The hosts of the addresses written in a link's text, like "Log in at paypal.com." */
function shownHosts(text: string): (string | null)[] {
  return text
    .replace(INVISIBLE, "")
    .split(/\s+/)
    .map((word) => word.replace(WORD_EDGE_PUNCTUATION, ""))
    .filter((word) => ADDRESS_WORD.test(word))
    .map((word) => hostOf(WEB_ADDRESS.test(word) ? word : `http://${word}`));
}

/**
 * Whether a link showing `text` that opens `href` could mislead about where
 * it goes, a common phishing trick: its text shows another address, or the
 * address names a user before its host.
 */
export function isMisleadingLink(text: string, href: string): boolean {
  if (isWebAddress(href) && hasUserInfo(href)) return true;
  const opened = hostOf(href);
  return shownHosts(text).some((shown) => shown === null || shown !== opened);
}

/** What to say before opening a link that [`isMisleadingLink`] flags. */
export function misleadingLinkWarning(text: string, href: string): string {
  if (isWebAddress(href) && hasUserInfo(href)) {
    const shown = authorityOf(href).split(/[@\\]/)[0].split(":")[0];
    return `This link goes to ${hostOf(href) ?? href}, not ${shown}.`;
  }
  return `Its text shows ${text.trim()}, but it opens ${href}. Open it only if you trust where it goes.`;
}
