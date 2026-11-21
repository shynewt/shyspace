# Design system

The desktop UI is built from a small set of shared components in `apps/desktop/src/ui/`. Screens compose them; they don't restyle them. To see every component, run the app in development and open it with `#gallery` (`dev/Gallery.tsx`).

## Look

Calm and warm: soft neutral surfaces, one accent (violet unless you pick another), rounded corners, gentle shadows. Light and dark follow the system setting.

- **Tokens** live in `src/styles.css` as CSS variables (`--bg`, `--surface`, `--surface-2/3`, `--border`, `--text`, `--text-muted/faint/decorative`, `--accent`, `--accent-soft/strong/text`, `--on-accent`, `--danger(-text)`, `--success(-text)`, `--warning(-text)`, `--shadow`, `--shadow-lg`). Tailwind exposes them as `bg-surface`, `text-muted`, `border-line`, `shadow-card`, `shadow-float` and so on. Never use raw colors in components; the logo is the one exception. Each theme's values are written once: dark ones under `@variant dark`, which matches a chosen dark theme, or the system's dark mode unless Light was chosen (`dark:` utilities follow the same rule).
- **Accent presets:** Settings › Appearance › Accent colour picks Violet (the default), Blue, Indigo, Teal, Pink or Graphite, per device (`lib/preferences.ts`, applied as `data-accent` on `<html>`, like `data-theme`). Each preset in `styles.css` sets `--accent`, `--accent-strong`, `--accent-soft`, `--accent-text` and, where white doesn't pass, `--on-accent`, for light and dark. Any element with `data-accent` shows that preset, which is how the swatches show their colour. There's no red, orange or green: those mean danger, warning and success. Text and icons on an accent fill use `text-on-accent`, never `text-white` (Graphite's dark fill is light). The focus ring and text selection follow the accent.
- **Contrast:** text meets WCAG AA (4.5:1) in both themes; `tests/ui/contrast.spec.ts` checks the token pairs, for every accent preset, so add a pair there when you add a token.
  - `text-muted` for secondary text, `text-faint` for small informational text (timestamps, hints, section labels). Both pass on `bg` and `surface`; only `text-muted` also passes on `surface-2`, so use it for text on `surface-2` fills (like `Kbd`).
  - `text-decorative` is for decoration only: an icon beside a label, a chevron, a spinner. Never for something someone has to read.
  - Semantic colors: the plain token (`bg-danger`, a dot, an icon) is for fills and graphics, and white text passes on its fills; the `-text` token (`text-danger-text`) is for text, also on its tints like `bg-danger/15`. The same goes for `accent` and `accent-text`, except that text on an accent fill is `on-accent`.
- **Type:** the system UI font, in one scale (`@theme` in `styles.css`): `text-2xs` 11 px (badges, keys, tiny labels), `text-xs` 12 px (hints, timestamps, section labels), `text-sm` 13 px (most interface text), `text-base` 14 px (message and preview text, the body default), `text-lg` 15 px (dialog and view titles), `text-xl` 20 px (emoji in pickers and buttons), `text-2xl` 26 px and `text-3xl` 38 px (onboarding and page titles). They replace Tailwind's sizes of the same names (font size only; line height stays as set), and `tests/ui/design-rules.spec.ts` fails on a hand-picked size like `text-[13px]`.
- **Shape:** `rounded-lg` (8 px) for controls, `rounded-xl` (12 px) for cards, `rounded-2xl` (16 px) for dialogs.

## Motion

All animation uses the presets in `ui/motion.ts`, through Motion:

| Preset | Use |
|---|---|
| `springs.snappy` | feedback: buttons, chips, sliding selection pills |
| `springs.smooth` | panels and dialogs entering, list items appearing |
| `springs.pop` | small playful pops: reactions, counters, copy confirmations |
| `rise`, `pop`, `fade` | enter and exit variants for content, floating surfaces, overlays |

Keep feedback under about 250 ms. Reduced motion is decided once, globally: `MotionConfig` in `main.tsx` follows the system unless Settings › Appearance › Reduce motion says On or Off. Don't override it in components.

## Components

| Component | For |
|---|---|
| `Button` | actions; variants `primary`, `secondary`, `ghost`, `danger`; sizes `sm`, `md`, `lg`; `isLoading` |
| `IconButton` | icon-only actions; `label` is required and becomes the tooltip and accessible name |
| `Input`, `TextArea`, `Field` | form controls; `Field` links its label to the control through a render function |
| `FormError` | why a form's last save failed, under the form; announced to screen readers |
| `Dialog` | modal surfaces (centered, or `placement="top"` like the search palette); never taller than the window, longer content scrolls inside; `hasCloseButton` for dialogs without their own Cancel or Done; pads its content and stacks it 16 px apart (`padding="none"` for edge-to-edge layouts like the palette and Settings); end with `DialogActions` for the buttons, the main one last |
| `Menu`, `Popover`, `Tooltip` | floating surfaces anchored to a trigger; a menu sets destructive actions apart below a line; `Popover` can hand focus back to a field (`returnFocusTo`) |
| `Card`, `SectionLabel`, `EmptyState` | grouping and empty screens |
| `Avatar`, `Badge`, `Kbd`, `Spinner`, `Logo` | small content pieces |
| `ListRow` | a clickable row in a list (Inbox, Later): `leading` avatar or icon, a small `meta` line, a one-line preview; `trailing` actions sit outside the button; `isHighlighted` keeps the hover background (while its menu is open) |
| `CountBadge` | a count pill ("3", "@2", "4 new"), `accent` or `danger`; takes motion props so it can pop in when the count changes |
| `Notice` | a callout inside a screen: `info` (read-only and other things to know), `warning` or `danger` (something to fix), with an optional title |
| `RelativeTime` | a past moment as short as stays clear ("Yesterday 4:41 PM"), the full date on hover |
| `SegmentedControl` | picking one of a few options: a radio group (arrow keys move the choice) named by `label`, sized to its options or `width="full"` |
| `CopyField` | codes and commands with a copy button |
| `EmojiPicker` | a searchable emoji grid (names and keywords), recently used first, inside a `Popover`; it and its data load on first open, ship with the app and are never fetched from the network |
| `ConfirmDialog` | asks before something that can't be undone (deletes), showing what it applies to; stays open showing the error if it fails |
| `Toasts` | transient messages above the message box, from `lib/toast.ts`: `showToast` (a success, or `"info"`), `showUndoToast`, `showReminderToast` (with Open) and `reportError` (announced as an alert) |

Dialogs, menus, popovers and tooltips use [Radix](https://www.radix-ui.com/primitives) primitives for focus handling and keyboard support, styled by us.

## Editor

Pages are markdown in a CodeMirror editor (`features/pages/`), under a title field that grows to fit; a new page starts in its title, and Enter moves on to the body. The text people and agents share is always plain markdown; the editor only changes how it looks (live preview, like Obsidian):

- Lines nobody is editing read as formatted text: marks (`#`, `**`, `_`, backticks, link brackets and addresses, `>`) are hidden, list dashes become bullets, `- [ ]` becomes a checkbox, `---` a divider, and a quote starting `[!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]` or `[!CAUTION]` (GitHub's callouts; `[!INFO]` is a note) a coloured callout with that label. Lines touched by your cursor or selection show their markdown, so you edit what is really there. While a mouse button is down nothing changes, so text doesn't jump under the pointer.
- Code blocks hide their ``` fences until you edit those lines, keep the language name as a small label, and colour the code by that language. Messages colour code the same way: one highlighter and the `tok-*` classes in `styles.css` (`lib/codeHighlight.ts`, CodeMirror's parsers loaded per language on first use).
- A table shows as a real table until the cursor enters it; clicking a cell puts the cursor in that cell's text, and while you edit, the table's markdown shows in monospace so the columns line up. Messages render tables and callouts too, with the same callout colours (`lib/callouts.ts`, `callout-<kind>` in `styles.css`).
- Page links (`shy://page/<id>`) and mentions are chips, also on the line being edited; the cursor steps over a chip as one unit. Page chips show the page's current title and open it on click. Mention chips open the person's profile card (click, or Ctrl/Cmd+Enter beside one), placed at the chip, without moving the cursor; Esc goes back to the editor. A mention of someone who isn't a member opens nothing.
- Web links show their address on hover and open with Ctrl/Cmd+click.
- Menus under the cursor: `/` (at the start of a line or after a space) for blocks, `@` for people, `[[` for pages. They open only when you type the trigger, never for another member's edit; arrows, Enter or Tab and Esc drive them while the editor keeps focus and points at the chosen option with `aria-activedescendant`. Each choice inserts the markdown you could type yourself.
- Keys write markdown: Ctrl/Cmd+B, I and E wrap the selection in bold, italic or code (again to undo it, or to step out of an empty pair); Ctrl/Cmd+K links the selection, and without one stays the app's search; Ctrl/Cmd+Enter checks off the item, opens the link, or shows the mentioned person's card at the cursor. Undo and redo only touch your own edits.

Rules for changing it:

- Decorations never change the document. A widget that acts (a checkbox) makes an ordinary edit, computed from the document as it is at that moment, never from a remembered position, so it stays right while others edit.
- `editor/markdownPreview.ts` decides what to hide or draw and is pure (tested in `tests/ui/markdown-preview.spec.ts`); `editor/livePreview.ts` turns that into decorations. Only the visible part of the page is decorated, so long pages stay fast. Tables are block widgets, which CodeMirror takes only from a state field, so `editor/tables.ts` draws them separately.
- Plugins may not hide a line break; keep each hidden range on one line.
- Styles live in `editor/theme.ts` and use the tokens; color only list markers, never whole list lines.

## Rules

- `@` in the message box suggests people, and once you type a letter, up to three pages whose title has a word starting with it, under a "Pages" label. A picked page shows as `@Title` while you write and is sent as its link, which reads as a page chip. Dragging a page from the sidebar onto the message box does the same at the cursor (`lib/pageDrag.ts`); dropped elsewhere, like into a page, it's the page's link.
- The message box offers emoji three ways, all from the app's own emoji set and sharing one "Recently used" list: the smiley button (inserts at the cursor), `:` and two or more letters (suggestions by shortcode or keyword, in the same keyboard-driven list as `@` mentions, `chat/useSuggestions.tsx`; not inside code, links or times like `10:30`), and "More emoji…" beside the quick reactions. The emoji data loads only on first use.
- Back and forward (`lib/history.ts`, wired up in `lib/useNavigation.ts`): changing channel, opening or closing a thread, opening a page, Inbox, Later or a search result each add a place; dialogs (Settings), the search palette, cards, scrolling and reopening the place you're in don't. Up to 50 places, in memory. Keys: Alt+← / Alt+→ (⌘[ / ⌘] on a Mac, where Option+arrows move by word) and the mouse's back and forward buttons, also while typing; keys the page editor already uses stay the editor's. Places that were deleted or can't be opened any more (including a private channel you were removed from) are skipped and forgotten, with a notice only when every place that way is gone; a thread whose first message was deleted opens its channel.
- Coming back to a channel opens it where you left it: through history always, and from the sidebar when it has nothing unread (otherwise at the new-messages line). Positions are a scroll anchor (`lib/scrollAnchor.ts`: the top visible message and its offset), never raw pixels, so they survive history loading and text rewrapping; at the newest message there's no anchor and the channel opens as usual. The app reopens at the last view (with its thread and anchor), the only part of the history kept on the device (`lib/lastView.ts`).
- Reading (`chat/useReadPosition.ts`): a message is read only once its bottom edge has been on screen while the window has focus, isn't hidden, and its channel (or thread) is the view you're in. The read position is the newest such message, only moves forward, and is saved within about 300 ms of moving, when you leave, and when the window loses focus or hides; a channel opened again right away uses the position just sent, even if the core's answer is still on its way. Threads keep their own position. Sidebar counts, @-badges and the Inbox all come from these positions; nothing keeps a separate count.
- The new-messages line goes above the first unread message as you open a channel and stays put for that visit; messages that arrive while the window can't be looked at get a line above the first of them. Esc marks the channel read, removes the line and both pills, and goes to the newest message.
- Pills over the list (`chat/UnreadPills.tsx`, primary `Button`s announced politely): "Jump to unread ↑" shows on opening when the first unread message is above the view, and goes for that visit once the line is on screen, it's clicked, or Esc. "N new messages ↓" shows only while you're scrolled up and others' messages arrive (N counts them), and goes at the bottom, on click, on sending, or on leaving.
- Following (`chat/useHeldScroll.ts`): content arriving keeps you at the newest message while you're there; scrolling up by any means (wheel, keys, dragging) stops that at once, and scrolling back down within 24 px resumes it. Otherwise the top visible message stays exactly where it is as anything loads or changes size, moved only by the difference so a fling carries on (the browser's own `overflow-anchor` is off, so WebKit and Chromium behave the same).
- Keyboard shortcuts are defined once in `lib/shortcuts.ts` (the shell handles them, the cheat sheet and menus show them). Use a modifier key: the message box usually has focus, so plain letters belong to typing.
- The logo lives in `docs/brand/`: `mark.svg` (the app icon and `ui/Logo`), `logo-light.svg` and `logo-dark.svg` (wordmarks). After changing the mark, copy it to `apps/desktop/app-icon.svg` and run `pnpm tauri icon app-icon.svg`.
- Need something new? First check whether an existing component (or a variant of it) fits. Add a component to `ui/` only when a third screen needs the same thing.
- New components go in `ui/`, get an entry in `ui/index.ts`, and appear in the gallery.
- `tests/ui/design-rules.spec.ts` keeps this from drifting: it fails on a hand-picked font size anywhere, and in `features/` on raw colours (`text-white`, palette colours like `bg-red-500`, hex, `rgb()`) and on native `<button>`s with a solid accent, danger, success or warning fill (use `Button`).
- Every interactive element works with the keyboard and has an accessible name.
- Focus rings are for keyboard use. After a click, focus that Radix moves (back to a menu's trigger, into a dialog) gets no ring: `lib/useInputModality.ts` marks how the app was last used. Controls that show on hover also show for keyboard focus with `keyboard-focus-within:`, not `focus-within:`, which stays on after a click.
- Saving: choices and toggles apply at once. Text forms have one Save button named for what it does ("Save profile"). Never mix the two in one form. A failed save keeps what the user typed and shows why with `FormError` (`lib/useFormAction.ts` does both).
- Settings is one fixed-size window: sections under "You" and under the workspace's name. Open it at a section from anywhere with `useWorkspace().openSettings({ section })`; every section is also in the command palette ("Settings: Notifications").
- The sidebar has two menus: the workspace's name at the top (Invite people, join requests, Workspace settings) and you at the bottom (Set status, Appear offline, Settings). Personal things that change often, like status, live in your menu, not in Settings.
- Under a thread's first message, one line sums up its replies (`chat/ThreadSummary.tsx`): up to three small avatars of the latest repliers (newest first), "N replies" in the accent (semibold with a "N new" badge while some are unread), the last reply's author and a plain one-line preview, and its time. The preview is cut first when space runs out; the count never wraps. The whole line is one button that opens the thread; hover gives it a `surface-2` background and a chevron.
- Status beside names: only the emoji, never the text, in message headers (once per group, so threads too), the member list and the `@` suggestions; hover or keyboard focus tells the whole status ("🌴 On holiday · until Fri 18:00", `members/StatusEmoji.tsx`). A status without an emoji shows nothing there. Ended statuses disappear on their own, live (`lib/members.ts`). Presence dots show only in the member list and the `@` suggestions, never in messages. Agents show their tag instead of a presence dot, and no status.
- Profile cards (`members/ProfileCard.tsx`): clicking, Enter or Space on a name or avatar in a message header, or on an @mention, opens a card as a popover with focus inside (on the card after a click, so no ring shows; on its first action after Enter or Space); Esc or clicking outside closes it and focus returns to the name. Pausing 600 ms on a header's name or avatar also opens it, without taking focus, and it closes 300 ms after the pointer leaves both the name and the card (WCAG 1.4.13); @mentions in text never open on hover. The card shows the avatar, name, Admin or Agent tag, presence (people only), and the status with when it ends; actions are Message (opens your DM with them, agents included, as a step in history), Mention (into the message box beside the message) and View in members. It's built on Radix Popover, not HoverCard, which the keyboard can't reach.
- A channel named in visible text uses `private/ChannelName.tsx`: # and the name, a lock for a private channel, or a DM's person (avatar and name), never a hand-written `#name`. Accessible labels and dialog titles use the `channelReference` string from `lib/channels.ts`.
- A warning uses a small triangle (`TriangleAlert`, `text-danger`) with an accessible name and a tooltip saying what's wrong. It marks the way to a problem that lasts until someone fixes it, like storage that can lose data (the "me" button, Storage in Settings, and a "me" menu item). Its notice in Settings has no dismiss button. Dots mean presence or unread, never a warning.
- Sidebar counts mean "this needs you" (unread messages, mentions, a reminder that's due): a coloured badge and a bold label. Things that are only waiting, like saved items, get no count.
