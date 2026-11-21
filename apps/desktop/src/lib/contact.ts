/**
 * Opening the ways to reach someone with the system's own handlers: mail for
 * an email address, the phone app for a number, the browser for a website.
 * Only these schemes are ever built here.
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

export function openExternal(url: string): void {
  openUrl(url).catch(reportError);
}
