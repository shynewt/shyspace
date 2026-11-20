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

Keep feedback under about 250 ms. Reduced-motion settings are respected globally (`MotionConfig reducedMotion="user"` in `main.tsx`); don't override it.

## Components

| Component | For |
|---|---|
| `Button` | actions; variants `primary`, `secondary`, `ghost`, `danger`; sizes `sm`, `md`, `lg`; `isLoading` |
| `IconButton` | icon-only actions; `label` is required and becomes the tooltip and accessible name |
| `Input`, `TextArea`, `Field` | form controls; `Field` links its label to the control through a render function |
| `Dialog` | modal surfaces (centered, or `placement="top"` like the search palette) |
| `Drawer` | side panels, like Members |
| `Menu`, `Popover`, `Tooltip` | floating surfaces anchored to a trigger; a menu sets destructive actions apart below a line |
| `Card`, `SectionLabel`, `EmptyState` | grouping and empty screens |
| `Avatar`, `Badge`, `Kbd`, `Spinner`, `Logo` | small content pieces |
| `SegmentedControl` | picking one of a few options: a radio group (arrow keys move the choice) named by `label`, sized to its options or `width="full"` |
| `CopyField` | codes and commands with a copy button |
| `ConfirmDialog` | asks before something that can't be undone (deletes), showing what it applies to; stays open showing the error if it fails |
| `Toasts` | transient messages above the message box, from `lib/toast.ts`: `showToast` (a success, or `"info"`), `showUndoToast`, `showReminderToast` (with Open) and `reportError` (announced as an alert) |

Dialogs, drawers, menus, popovers and tooltips use [Radix](https://www.radix-ui.com/primitives) primitives for focus handling and keyboard support, styled by us.

## Rules

- Keyboard shortcuts are defined once in `lib/shortcuts.ts` (the shell handles them, the cheat sheet and menus show them). Use a modifier key: the message box usually has focus, so plain letters belong to typing.
- The logo lives in `docs/brand/`: `mark.svg` (the app icon and `ui/Logo`), `logo-light.svg` and `logo-dark.svg` (wordmarks). After changing the mark, copy it to `apps/desktop/app-icon.svg` and run `pnpm tauri icon app-icon.svg`.
- Need something new? First check whether an existing component (or a variant of it) fits. Add a component to `ui/` only when a third screen needs the same thing.
- New components go in `ui/`, get an entry in `ui/index.ts`, and appear in the gallery.
- Every interactive element works with the keyboard and has an accessible name.
- Focus rings are for keyboard use. After a click, focus that Radix moves (back to a menu's trigger, into a dialog) gets no ring: `lib/useInputModality.ts` marks how the app was last used. Controls that show on hover also show for keyboard focus with `keyboard-focus-within:`, not `focus-within:`, which stays on after a click.
- Sidebar counts mean "this needs you" (unread messages, mentions, a reminder that's due): a coloured badge and a bold label. Things that are only waiting, like saved items, get no count.
