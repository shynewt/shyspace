/**
 * Checking your own profile details before they're saved, by the rules the
 * core applies to everyone's (shy-core's profile.rs), so a mistake shows
 * beside its field instead of failing the save.
 */
import type { ProfileDetails } from "./api";
import { characterCount, PROFILE_LIMITS } from "./limits";

const SCHEME = /^[a-z][a-z0-9+.-]*:(?!\d)/i;
const WEB_SCHEME = /^https?:\/\//i;
/** Whitespace, controls, and the invisible characters the core drops (zero-width, direction marks, BOM). */
const UNSAFE_IN_ADDRESS = /[\s\p{Cc}­؜᠎​-‏‪-‮⁠-⁯﻿]/u;
const NOT_IN_EMAIL = /[?#%/\\<>"'()[\],;:@]/;

/** What someone typed as a website, with `https://` added when they left the scheme out ("example.com"). */
export function withWebScheme(typed: string): string {
  const website = typed.trim();
  if (!website || SCHEME.test(website)) return website;
  return `https://${website}`;
}

export function websiteProblem(website: string): string | null {
  if (!website) return null;
  const host = website.replace(WEB_SCHEME, "").split(/[/?#]/)[0];
  if (!WEB_SCHEME.test(website) || !host || UNSAFE_IN_ADDRESS.test(website))
    return "Use a web address like example.com.";
  if (characterCount(website) > PROFILE_LIMITS.website) return `Use at most ${PROFILE_LIMITS.website} characters.`;
  return null;
}

export function emailProblem(email: string): string | null {
  if (!email) return null;
  const [local, domain, ...rest] = email.split("@");
  const isAllowed = (part: string) => !UNSAFE_IN_ADDRESS.test(part) && !NOT_IN_EMAIL.test(part);
  const labels = domain?.split(".") ?? [];
  const isWellFormed =
    rest.length === 0 &&
    Boolean(local) &&
    isAllowed(local) &&
    isAllowed(domain ?? "") &&
    labels.length >= 2 &&
    labels.every((label) => label !== "");
  if (!isWellFormed || characterCount(email) > PROFILE_LIMITS.email) return "Use an address like name@example.com.";
  return null;
}

export function phoneProblem(phone: string): string | null {
  if (!phone) return null;
  if (!/^[0-9 +().-]+$/.test(phone) || !/[0-9]/.test(phone)) return "Use digits, spaces and + ( ) - . only.";
  return null;
}

/** The four things that tell people who you are and when you're around. */
export function completedProfileSteps(details: ProfileDetails): number {
  return [
    details.title !== "",
    details.pronouns !== "" || details.bio !== "",
    details.time_zone !== "",
    details.working_hours !== null,
  ].filter(Boolean).length;
}

export const PROFILE_STEPS = 4;
