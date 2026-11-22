import { type FormEvent, type ReactNode, type RefObject, useCallback, useEffect, useRef, useState } from "react";
import { api, type ProfileDetails } from "../../lib/api";
import { characterCount, cutToCharacters, MAX_PROFILE_NAME_CHARS, PROFILE_LIMITS } from "../../lib/limits";
import { emailProblem, phoneProblem, websiteProblem, withWebScheme } from "../../lib/profileFields";
import type { SettingsTarget } from "../../lib/settings";
import { showToast } from "../../lib/toast";
import { useFormAction } from "../../lib/useFormAction";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { Button, Field, FormError, Input, TextArea } from "../../ui";
import { ProfileCompletion } from "./ProfileCompletion";
import { TimeZonePicker } from "./TimeZonePicker";
import { WorkingHoursField, workingHoursProblem } from "./WorkingHoursField";

/** Where the bio starts showing how close it is to the limit. */
const SHOW_BIO_COUNT_FROM = PROFILE_LIMITS.bio - 60;

type Draft = { name: string; details: ProfileDetails };
/** Fields checked beyond their length, in the order they appear. */
const CHECKED_FIELDS = ["email", "phone", "website", "working_hours"] as const;
type CheckedField = (typeof CHECKED_FIELDS)[number];
type TextField = "title" | "pronouns" | "location" | "email" | "phone" | "website";

function problemOf(field: CheckedField, details: ProfileDetails): string | null {
  switch (field) {
    case "email":
      return emailProblem(details.email);
    case "phone":
      return phoneProblem(details.phone);
    case "website":
      return websiteProblem(details.website);
    case "working_hours":
      return workingHoursProblem(details.working_hours);
  }
}

function sameDetails(left: ProfileDetails, right: ProfileDetails): boolean {
  const [leftHours, rightHours] = [left.working_hours, right.working_hours];
  const isSameHours =
    leftHours === rightHours ||
    (leftHours !== null &&
      rightHours !== null &&
      leftHours.days === rightHours.days &&
      leftHours.start_minute === rightHours.start_minute &&
      leftHours.end_minute === rightHours.end_minute);
  const texts = ["title", "pronouns", "bio", "email", "phone", "website", "location", "time_zone"] as const;
  return isSameHours && texts.every((key) => left[key] === right[key]);
}

/**
 * Your profile, as everyone in the workspace sees it: who you are, how to
 * reach you, and when you're around. One form with one Save. A mistake shows
 * beside its field once you leave the field, or when you save.
 */
export function ProfileSettings({ target }: { target: SettingsTarget }) {
  const { session } = useWorkspace();
  const loadMembers = useCallback(() => api.members(), []);
  const members = useLive(loadMembers, (change) => change.kind === "members");
  const me = members?.find((member) => member.id === session.me);
  const saved: Draft | undefined = me && { name: me.name, details: me.details };
  const savedKey = JSON.stringify(saved ?? null);
  const [draft, setDraft] = useState<Draft | undefined>(saved);
  const [draftFrom, setDraftFrom] = useState(savedKey);
  const [checked, setChecked] = useState<ReadonlySet<CheckedField>>(new Set());
  const form = useRef<HTMLFormElement>(null);
  if (draftFrom !== savedKey) {
    setDraftFrom(savedKey);
    setDraft(saved);
  }
  useFocusField(form, target, draft !== undefined);

  const save = useFormAction(async () => {
    if (!draft || !saved) return;
    if (draft.name.trim() !== saved.name) await api.setProfile(draft.name);
    if (!sameDetails(draft.details, saved.details)) await api.setProfileDetails(draft.details);
    showToast("Your profile is updated for everyone.");
  });

  if (!draft || !saved) return null;
  const details = draft.details;
  const problems = CHECKED_FIELDS.filter((field) => problemOf(field, details) !== null);
  const shownProblem = (field: CheckedField) => (checked.has(field) ? problemOf(field, details) : null);
  const check = (field: CheckedField) => setChecked((before) => new Set(before).add(field));
  const isChanged = draft.name !== saved.name || !sameDetails(details, saved.details);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (problems.length === 0) return save.run();
    setChecked(new Set([...checked, ...problems]));
    form.current?.querySelector<HTMLElement>(`[data-field="${problems[0]}"]`)?.focus();
  };
  const setDetail = <Key extends keyof ProfileDetails>(key: Key, value: ProfileDetails[Key]) =>
    setDraft({ ...draft, details: { ...details, [key]: value } });
  const textProps = (key: TextField) => ({
    "data-field": key,
    value: details[key],
    onChange: (event: React.ChangeEvent<HTMLInputElement>) =>
      setDetail(key, cutToCharacters(event.target.value, PROFILE_LIMITS[key])),
  });
  const bioLength = characterCount(details.bio);

  return (
    <>
      <ProfileCompletion details={saved.details} />
      <form ref={form} onSubmit={submit} noValidate className="flex flex-col gap-6">
        <Group title="About you">
          <Field label="Display name" hint="Everyone in the workspace sees this name.">
            {(id, description) => (
              <Input
                id={id}
                {...description}
                data-field="name"
                value={draft.name}
                maxLength={MAX_PROFILE_NAME_CHARS}
                onChange={(event) => setDraft({ ...draft, name: event.target.value })}
              />
            )}
          </Field>
          <div className="grid grid-cols-2 gap-3">
            <Field label="Pronouns" hint="For example she/her, he/him or they/them.">
              {(id, description) => <Input id={id} {...description} {...textProps("pronouns")} />}
            </Field>
            <Field label="Title" hint="Your role, like Product designer.">
              {(id, description) => <Input id={id} {...description} {...textProps("title")} />}
            </Field>
          </div>
          <Field
            label="Bio"
            hint={
              bioLength >= SHOW_BIO_COUNT_FROM ? (
                <span className="text-warning-text tabular-nums">
                  {bioLength} / {PROFILE_LIMITS.bio}
                </span>
              ) : (
                "A few words about you. Links work."
              )
            }
          >
            {(id, description) => (
              <TextArea
                id={id}
                {...description}
                data-field="bio"
                rows={3}
                value={details.bio}
                onChange={(event) => setDetail("bio", cutToCharacters(event.target.value, PROFILE_LIMITS.bio))}
              />
            )}
          </Field>
        </Group>

        <Group title="Contact">
          <div className="grid grid-cols-2 gap-3">
            <Field label="Email" hint="Opens in their mail app." error={shownProblem("email")}>
              {(id, description) => (
                <Input
                  id={id}
                  {...description}
                  {...textProps("email")}
                  type="email"
                  autoComplete="email"
                  onBlur={() => check("email")}
                />
              )}
            </Field>
            <Field label="Phone" hint="Digits, spaces and + ( ) - ." error={shownProblem("phone")}>
              {(id, description) => (
                <Input
                  id={id}
                  {...description}
                  {...textProps("phone")}
                  type="tel"
                  autoComplete="tel"
                  onBlur={() => check("phone")}
                />
              )}
            </Field>
          </div>
          <Field label="Website" hint="Opens in their browser." error={shownProblem("website")}>
            {(id, description) => (
              <Input
                id={id}
                {...description}
                {...textProps("website")}
                type="url"
                autoComplete="url"
                placeholder="example.com"
                onBlur={() => {
                  setDetail("website", withWebScheme(details.website));
                  check("website");
                }}
              />
            )}
          </Field>
        </Group>

        <Group title="Time and place">
          <Field label="Time zone" hint="Shows people your local time.">
            {(id, description) => (
              <TimeZonePicker
                id={id}
                fieldName="time_zone"
                value={details.time_zone}
                onChange={(zone) => setDetail("time_zone", zone)}
                description={description}
              />
            )}
          </Field>
          <WorkingHoursField
            value={details.working_hours}
            onChange={(hours) => {
              setDetail("working_hours", hours);
              check("working_hours");
            }}
            error={shownProblem("working_hours")}
          />
          <Field label="Location" hint="Where you work, like Berlin office or Remote, Lisbon.">
            {(id, description) => <Input id={id} {...description} {...textProps("location")} />}
          </Field>
        </Group>

        <div className="flex flex-col gap-3">
          <FormError message={save.error} />
          <Button
            type="submit"
            variant="primary"
            isLoading={save.isBusy}
            disabled={!draft.name.trim() || !isChanged}
            // Keeps focus in the field while the button is pressed: leaving it shows its error, which can push
            // the button down before the press ends, and the click would be lost. Saving checks every field anyway.
            onMouseDown={(event) => event.preventDefault()}
            className="self-start"
          >
            Save profile
          </Button>
          <p className="text-xs leading-relaxed text-muted">
            Everything filled in here is visible to everyone in the workspace. Your status and appearing offline are in
            the menu under your name, at the bottom of the sidebar.
          </p>
        </div>
      </form>
    </>
  );
}

function Group({ title, children }: { title: string; children: ReactNode }) {
  return (
    <fieldset className="flex min-w-0 flex-col gap-3">
      <legend className="mb-3 text-xs font-semibold tracking-wide text-faint uppercase">{title}</legend>
      {children}
    </fieldset>
  );
}

/** Focuses the field `target` names once the form is there, after the dialog's own first focus. */
function useFocusField(form: RefObject<HTMLFormElement | null>, target: SettingsTarget, isReady: boolean) {
  useEffect(() => {
    if (!target.field || !isReady) return;
    const selector = `[data-field="${target.field}"]`;
    const frame = requestAnimationFrame(() => form.current?.querySelector<HTMLElement>(selector)?.focus());
    return () => cancelAnimationFrame(frame);
  }, [form, target, isReady]);
}
