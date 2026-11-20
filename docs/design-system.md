# Design system

The desktop UI is built from a small set of shared components in `apps/desktop/src/ui/`. Screens compose them; they don't restyle them. To see every component, run the app in development and open it with `#gallery` (`dev/Gallery.tsx`).

## Look

Calm and warm: soft neutral surfaces, one violet accent, rounded corners, gentle shadows. Light and dark follow the system setting.

- **Tokens** live in `src/styles.css` as CSS variables (`--bg`, `--surface`, `--surface-2/3`, `--border`, `--text`, `--text-muted/faint/decorative`, `--accent`, `--accent-soft/strong/text`, `--danger(-text)`, `--success(-text)`, `--warning(-text)`, `--shadow`, `--shadow-lg`). Tailwind exposes them as `bg-surface`, `text-muted`, `border-line`, `shadow-card`, `shadow-float` and so on. Never use raw colors in components; the logo is the one exception. Each theme's values are written once: dark ones under `@variant dark`, which matches a chosen dark theme, or the system's dark mode unless Light was chosen (`dark:` utilities follow the same rule).
- **Contrast:** text meets WCAG AA (4.5:1) in both themes; `tests/ui/contrast.spec.ts` checks the token pairs, so add a pair there when you add a token.
  - `text-muted` for secondary text, `text-faint` for small informational text (timestamps, hints, section labels). Both pass on `bg` and `surface`; only `text-muted` also passes on `surface-2`, so use it for text on `surface-2` fills (like `Kbd`).
  - `text-decorative` is for decoration only: an icon beside a label, a chevron, a spinner. Never for something someone has to read.
  - Semantic colors: the plain token (`bg-danger`, a dot, an icon) is for fills and graphics, and white text passes on its fills; the `-text` token (`text-danger-text`) is for text, also on its tints like `bg-danger/15`. The same goes for `accent` and `accent-text`.
- **Type:** the system UI font, 13–15 px for interface text, larger only for page titles.
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
| `Dialog` | modal surfaces (centered, or `placement="top"` like the search palette); never taller than the window, longer content scrolls inside; `hasCloseButton` for dialogs without their own Cancel or Done |
| `Menu`, `Popover`, `Tooltip` | floating surfaces anchored to a trigger; a menu sets destructive actions apart below a line; `Popover` can hand focus back to a field (`returnFocusTo`) |
| `Card`, `SectionLabel`, `EmptyState` | grouping and empty screens |
| `Avatar`, `Badge`, `Kbd`, `Spinner`, `Logo` | small content pieces |
| `SegmentedControl` | picking one of a few options: a radio group (arrow keys move the choice) named by `label`, sized to its options or `width="full"` |
| `CopyField` | codes and commands with a copy button |
| `EmojiPicker` | a searchable emoji grid (names and keywords), recently used first, inside a `Popover`; it and its data load on first open, ship with the app and are never fetched from the network |
| `ConfirmDialog` | asks before something that can't be undone (deletes), showing what it applies to; stays open showing the error if it fails |
| `Toasts` | transient messages above the message box, from `lib/toast.ts`: `showToast` (a success, or `"info"`), `showUndoToast`, `showReminderToast` (with Open) and `reportError` (announced as an alert) |

Dialogs, menus, popovers and tooltips use [Radix](https://www.radix-ui.com/primitives) primitives for focus handling and keyboard support, styled by us.

## Editor

Pages are markdown in a CodeMirror editor (`features/pages/`), under a title field that grows to fit; a new page starts in its title, and Enter moves on to the body. The text people and agents share is always plain markdown; the editor only changes how it looks (live preview, like Obsidian):

- Lines nobody is editing read as formatted text: marks (`#`, `**`, `_`, backticks, link brackets and addresses, `>`) are hidden, list dashes become bullets, `- [ ]` becomes a checkbox, `---` a divider. Lines touched by your cursor or selection show their markdown, so you edit what is really there. While a mouse button is down nothing changes, so text doesn't jump under the pointer.
- Page links (`shy://page/<id>`) and mentions are chips, also on the line being edited; the cursor steps over a chip as one unit. Page chips show the page's current title and open it on click.
- Web links show their address on hover and open with Ctrl/Cmd+click.
- Menus under the cursor: `/` (at the start of a line or after a space) for blocks, `@` for people, `[[` for pages. They open only when you type the trigger, never for another member's edit; arrows, Enter or Tab and Esc drive them while the editor keeps focus and points at the chosen option with `aria-activedescendant`. Each choice inserts the markdown you could type yourself.
- Keys write markdown: Ctrl/Cmd+B, I and E wrap the selection in bold, italic or code (again to undo it, or to step out of an empty pair); Ctrl/Cmd+K links the selection, and without one stays the app's search; Ctrl/Cmd+Enter checks off the item or opens the link at the cursor. Undo and redo only touch your own edits.

Rules for changing it:

- Decorations never change the document. A widget that acts (a checkbox) makes an ordinary edit, computed from the document as it is at that moment, never from a remembered position, so it stays right while others edit.
- `editor/markdownPreview.ts` decides what to hide or draw and is pure (tested in `tests/ui/markdown-preview.spec.ts`); `editor/livePreview.ts` turns that into decorations. Only the visible part of the page is decorated, so long pages stay fast.
- Plugins may not hide a line break; keep each hidden range on one line.
- Styles live in `editor/theme.ts` and use the tokens; color only list markers, never whole list lines.

## Rules

- The message box offers emoji three ways, all from the app's own emoji set and sharing one "Recently used" list: the smiley button (inserts at the cursor), `:` and two or more letters (suggestions by shortcode or keyword, in the same keyboard-driven list as `@` mentions, `chat/useSuggestions.tsx`; not inside code, links or times like `10:30`), and "More emoji…" beside the quick reactions. The emoji data loads only on first use.
- Back and forward (`lib/history.ts`, wired up in `lib/useNavigation.ts`): changing channel, opening or closing a thread, opening a page, Inbox, Later or a search result each add a place; dialogs (Settings), the search palette, cards, scrolling and reopening the place you're in don't. Up to 50 places, in memory. Keys: Alt+← / Alt+→ (⌘[ / ⌘] on a Mac, where Option+arrows move by word) and the mouse's back and forward buttons, also while typing; keys the page editor already uses stay the editor's. Places that were deleted or can't be opened any more are skipped and forgotten, with a notice only when every place that way is gone; a thread whose first message was deleted opens its channel.
- Coming back to a channel opens it where you left it: through history always, and from the sidebar when it has nothing unread (otherwise at the new-messages line). Positions are a scroll anchor (`lib/scrollAnchor.ts`: the top visible message and its offset), never raw pixels, so they survive history loading and text rewrapping; at the newest message there's no anchor and the channel opens as usual. The app reopens at the last view (with its thread and anchor), the only part of the history kept on the device (`lib/lastView.ts`).
- Keyboard shortcuts are defined once in `lib/shortcuts.ts` (the shell handles them, the cheat sheet and menus show them). Use a modifier key: the message box usually has focus, so plain letters belong to typing.
- The logo lives in `docs/brand/`: `mark.svg` (the app icon and `ui/Logo`), `logo-light.svg` and `logo-dark.svg` (wordmarks). After changing the mark, copy it to `apps/desktop/app-icon.svg` and run `pnpm tauri icon app-icon.svg`.
- Need something new? First check whether an existing component (or a variant of it) fits. Add a component to `ui/` only when a third screen needs the same thing.
- New components go in `ui/`, get an entry in `ui/index.ts`, and appear in the gallery.
- Every interactive element works with the keyboard and has an accessible name.
- Focus rings are for keyboard use. After a click, focus that Radix moves (back to a menu's trigger, into a dialog) gets no ring: `lib/useInputModality.ts` marks how the app was last used. Controls that show on hover also show for keyboard focus with `keyboard-focus-within:`, not `focus-within:`, which stays on after a click.
- Saving: choices and toggles apply at once. Text forms have one Save button named for what it does ("Save profile"). Never mix the two in one form. A failed save keeps what the user typed and shows why with `FormError` (`lib/useFormAction.ts` does both).
- Settings is one fixed-size window: sections under "You" and under the workspace's name. Open it at a section from anywhere with `useWorkspace().openSettings({ section })`; every section is also in the command palette ("Settings: Notifications").
- The sidebar has two menus: the workspace's name at the top (Invite people, join requests, Workspace settings) and you at the bottom (Set status, Appear offline, Settings). Personal things that change often, like status, live in your menu, not in Settings.
- A warning uses a small triangle (`TriangleAlert`, `text-danger`) with an accessible name and a tooltip saying what's wrong. It marks the way to a problem that lasts until someone fixes it, like storage that can lose data (the "me" button, Storage in Settings, and a "me" menu item). Its notice in Settings has no dismiss button. Dots mean presence or unread, never a warning.
- Sidebar counts mean "this needs you" (unread messages, mentions, a reminder that's due): a coloured badge and a bold label. Things that are only waiting, like saved items, get no count.
