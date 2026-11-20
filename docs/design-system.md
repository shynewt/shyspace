# Design system

The desktop UI is built from a small set of shared components in `apps/desktop/src/ui/`. Screens compose them; they don't restyle them. To see every component, run the app in development and open it with `#gallery` (`dev/Gallery.tsx`).

## Look

Calm and warm: soft neutral surfaces, one violet accent, rounded corners, gentle shadows. Light and dark follow the system setting.

- **Tokens** live in `src/styles.css` as CSS variables (`--bg`, `--surface`, `--surface-2/3`, `--border`, `--text`, `--text-muted/faint`, `--accent`, `--accent-soft/strong/text`, `--danger`, `--success`, `--warning`, `--shadow`, `--shadow-lg`). Tailwind exposes them as `bg-surface`, `text-muted`, `border-line`, `shadow-card`, `shadow-float` and so on. Never use raw colors in components; the logo is the one exception.
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
| `Menu`, `Popover`, `Tooltip` | floating surfaces anchored to a trigger |
| `Card`, `SectionLabel`, `EmptyState` | grouping and empty screens |
| `Avatar`, `Badge`, `Kbd`, `Spinner`, `Logo` | small content pieces |
| `SegmentedControl` | picking one of a few modes |
| `CopyField` | codes and commands with a copy button |
| `ConfirmDialog` | asks before something that can't be undone (deletes); stays open showing the error if it fails |
| `Toasts` | transient messages; trigger with `showToast` / `reportError` from `lib/toast.ts` |

Dialogs, drawers, menus, popovers and tooltips use [Radix](https://www.radix-ui.com/primitives) primitives for focus handling and keyboard support, styled by us.

## Rules

- Keyboard shortcuts are defined once in `lib/shortcuts.ts` (the shell handles them, the cheat sheet and menus show them). Use a modifier key: the message box usually has focus, so plain letters belong to typing.
- The logo lives in `docs/brand/`: `mark.svg` (the app icon and `ui/Logo`), `logo-light.svg` and `logo-dark.svg` (wordmarks). After changing the mark, copy it to `apps/desktop/app-icon.svg` and run `pnpm tauri icon app-icon.svg`.
- Need something new? First check whether an existing component (or a variant of it) fits. Add a component to `ui/` only when a third screen needs the same thing.
- New components go in `ui/`, get an entry in `ui/index.ts`, and appear in the gallery.
- Every interactive element works with the keyboard and has an accessible name.
- Sidebar counts: a coloured badge and a bold label mean "this needs you" (unread messages, mentions, a reminder that's due). Things that are only waiting, like saved items, get a grey number (`NavItem`'s `quietCount`).
