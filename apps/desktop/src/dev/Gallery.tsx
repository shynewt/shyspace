import { Bell, Plus, Settings, Trash2 } from "lucide-react";
import { type ReactNode, useState } from "react";
import { PALETTE_NAMES, PALETTE_SWATCHES } from "../lib/palettes";
import { PALETTES, type Palette } from "../lib/preferences";
import { showToast } from "../lib/toast";
import {
  Avatar,
  AvatarStack,
  Badge,
  Button,
  Card,
  ConfirmDialog,
  ContextMenu,
  CopyButton,
  CopyField,
  CountBadge,
  DayDivider,
  Dialog,
  DialogActions,
  EmojiPicker,
  EmptyState,
  Field,
  FormError,
  IconButton,
  InlineNotice,
  Input,
  Kbd,
  ListRow,
  Logo,
  Menu,
  Notice,
  Popover,
  RelativeTime,
  SectionLabel,
  SegmentedControl,
  Select,
  Spinner,
  Switch,
  TextArea,
  ThemeSwatches,
  Tooltip,
} from "../ui";

/** Every design-system component in one place. Open the app with #gallery in development. */
export default function Gallery() {
  const [isDialogOpen, setDialogOpen] = useState(false);
  const [isConfirmOpen, setConfirmOpen] = useState(false);
  const [segment, setSegment] = useState<"one" | "two" | "three">("one");
  const [galleryPalette, setGalleryPalette] = useState<Palette>("fjord");
  const [isSwitchOn, setSwitchOn] = useState(true);
  const [isSpeaking, setSpeaking] = useState(true);
  return (
    <div className="h-full overflow-y-auto bg-surface">
      <div className="mx-auto flex max-w-[860px] flex-col gap-8 px-8 py-10">
        <header className="flex items-center gap-3">
          <Logo size={32} />
          <h1 className="text-xl font-semibold tracking-tight">shyspace design system</h1>
        </header>

        <Example title="Buttons">
          <Button variant="primary" icon={<Plus size={15} />}>
            Primary
          </Button>
          <Button>Secondary</Button>
          <Button variant="ghost">Ghost</Button>
          <Button variant="danger" icon={<Trash2 size={14} />}>
            Danger
          </Button>
          <Button variant="primary" isLoading>
            Loading
          </Button>
          <Button size="sm">Small</Button>
          <Button size="lg" variant="primary">
            Large
          </Button>
          <Button disabled>Disabled</Button>
        </Example>

        <Example title="Icon buttons and tooltips">
          <IconButton label="Notifications" icon={<Bell size={15} />} />
          <IconButton label="Settings" icon={<Settings size={15} />} isActive />
          <IconButton label="Small" size="sm" icon={<Plus size={13} />} />
          <Tooltip label="Tooltips work on anything focusable">
            <Button>Hover me</Button>
          </Tooltip>
        </Example>

        <Example title="Fields">
          <div className="grid w-full grid-cols-2 gap-3">
            <Field label="Name" hint="Shown to everyone in the workspace.">
              {(id) => <Input id={id} placeholder="Ada Lovelace" />}
            </Field>
            <Field label="Notes">{(id) => <TextArea id={id} rows={2} placeholder="Write something…" />}</Field>
            <Field label="Email" hint="Checked once you leave the field." error="Use an address like name@example.com.">
              {(id, description) => <Input id={id} {...description} defaultValue="ada@example" />}
            </Field>
            <Field label="Start">{(id) => <GallerySelect id={id} />}</Field>
            <FormError message="The bucket refused these keys: 403 Forbidden" />
          </div>
        </Example>

        <Example title="Selection">
          <Switch label="An on/off setting" checked={isSwitchOn} onChange={setSwitchOn} />
          <SegmentedControl
            label="Example choice"
            options={[
              { value: "one", label: "One" },
              { value: "two", label: "Two" },
              { value: "three", label: "Three" },
            ]}
            value={segment}
            onChange={setSegment}
          />
          <div className="w-full max-w-[420px]">
            <SegmentedControl
              label="Example full-width choice"
              options={[
                { value: "one", label: "One" },
                { value: "two", label: "Two" },
                { value: "three", label: "Three" },
              ]}
              value={segment}
              onChange={setSegment}
              width="full"
            />
          </div>
          <div className="w-full max-w-[460px]">
            <ThemeSwatches
              label="Example theme"
              options={PALETTES.map((palette) => ({
                value: palette,
                label: PALETTE_NAMES[palette],
                detail: palette === "shy" ? "the workspace's default" : undefined,
                ...PALETTE_SWATCHES[palette],
              }))}
              value={galleryPalette}
              onChange={setGalleryPalette}
            />
          </div>
        </Example>

        <Example title="Floating surfaces">
          <Button onClick={() => setDialogOpen(true)}>Open dialog</Button>
          <Button onClick={() => setConfirmOpen(true)}>Confirm a delete</Button>
          <ContextMenu items={[{ label: "Copy image", onSelect: () => showToast("Copy image") }]}>
            <span className="rounded-lg border border-dashed border-line px-3 py-2 text-sm text-muted">
              Right-click here
            </span>
          </ContextMenu>
          <Menu
            trigger={<Button>Open menu</Button>}
            items={[
              { label: "Rename", onSelect: () => showToast("Rename") },
              {
                label: "Notifications",
                icon: <Bell size={14} />,
                choices: [
                  { label: "Every message", isChosen: true, onSelect: () => showToast("Every message") },
                  { label: "Mentions only", isChosen: false, onSelect: () => showToast("Mentions only") },
                ],
              },
              { label: "Delete", onSelect: () => showToast("Delete"), isDestructive: true, icon: <Trash2 size={14} /> },
            ]}
          />
          <Popover trigger={<Button>Open popover</Button>}>
            <div className="p-3 text-sm">Anchored to its trigger.</div>
          </Popover>
          <Popover trigger={<Button>Pick an emoji</Button>} side="bottom" align="start">
            <EmojiPicker onSelect={(emoji) => showToast(`Picked ${emoji}`)} />
          </Popover>
          <Button onClick={() => showToast("Saved")}>Show toast</Button>
        </Example>

        <Example title="Content">
          <Avatar id="a" name="Ada Lovelace" />
          <Avatar id="b" name="Claude" isBot />
          <button type="button" aria-label="Toggle speaking" onClick={() => setSpeaking(!isSpeaking)}>
            <Avatar id="c" name="Grace Hopper" isSpeaking={isSpeaking} />
          </button>
          <AvatarStack
            avatars={["Ada Lovelace", "Grace Hopper", "Linus", "Mia", "Sam"].map((name) => ({
              key: name,
              avatar: <Avatar id={name} name={name} size={18} />,
            }))}
          />
          <Badge tone="accent">Agent</Badge>
          <Badge>neutral</Badge>
          <Badge tone="warning">warning</Badge>
          <Badge tone="danger">danger</Badge>
          <Badge tone="success">success</Badge>
          <CountBadge>3</CountBadge>
          <CountBadge tone="danger">@2</CountBadge>
          <CountBadge tone="quiet">2</CountBadge>
          <RelativeTime ms={Date.now() - 86_400_000} className="text-xs text-faint" />
          <Kbd>Ctrl K</Kbd>
          <Spinner />
        </Example>

        <Example title="List rows">
          <div className="w-full">
            <DayDivider label="Today" />
            <ListRow
              onClick={() => showToast("Opened")}
              leading={<Avatar id="a" name="Ada Lovelace" size={32} />}
              meta={
                <>
                  <b className="font-semibold text-ink">Ada Lovelace</b> mentioned you in general
                  <RelativeTime ms={Date.now() - 3_600_000} className="ml-auto shrink-0 text-faint" />
                </>
              }
            >
              Could you look at the launch plan?
            </ListRow>
            <ListRow
              onClick={() => showToast("Opened")}
              leading={<Avatar id="b" name="Grace Hopper" size={32} />}
              meta={<b className="font-semibold text-ink">Grace Hopper</b>}
              footer={<span>2 replies</span>}
            >
              A row with a footer line under its preview.
            </ListRow>
          </div>
        </Example>

        <Example title="Notices">
          <div className="flex w-full flex-col gap-3">
            <Notice>
              <p>You can still read this conversation, but not write in it.</p>
            </Notice>
            <Notice tone="warning" title="Partly protected">
              <p>Warnings say what's wrong and how to fix it.</p>
            </Notice>
            <Notice tone="danger" title="Your storage can lose data">
              <p>Dangers are for problems that put data at risk.</p>
            </Notice>
            <div className="flex w-64 flex-col gap-2">
              <InlineNotice tone="warning" action={{ label: "Choose another microphone", onClick: () => {} }}>
                Inline notices fit narrow places, with one fix.
              </InlineNotice>
              <InlineNotice tone="danger">Or none, when the fix is outside the app.</InlineNotice>
            </div>
          </div>
        </Example>

        <Example title="Cards and copy fields">
          <div className="grid w-full grid-cols-2 gap-3">
            <Card>
              <SectionLabel>Plain card</SectionLabel>
              <CopyField value="shy3_example-invite-code" />
              <div className="mt-2 flex items-center gap-2 text-sm">
                ada@example.com
                <CopyButton value="ada@example.com" label="Copy email address" />
              </div>
            </Card>
            <Card highlighted>
              <SectionLabel>Highlighted card</SectionLabel>
              <p className="text-sm text-muted">For things that need attention, like join requests.</p>
            </Card>
          </div>
        </Example>

        <Example title="Empty state">
          <div className="h-[220px] w-full rounded-xl border border-dashed border-line">
            <EmptyState
              icon={<Bell size={22} />}
              title="Nothing here yet"
              action={<Button variant="primary">Do something</Button>}
            >
              Empty states explain what will appear and how to start.
            </EmptyState>
          </div>
        </Example>
      </div>

      <Dialog
        open={isDialogOpen}
        onOpenChange={setDialogOpen}
        title="A dialog"
        description="Focus is trapped inside; Escape closes it."
      >
        <DialogActions>
          <Button variant="ghost" onClick={() => setDialogOpen(false)}>
            Cancel
          </Button>
          <Button variant="primary" onClick={() => setDialogOpen(false)}>
            Confirm
          </Button>
        </DialogActions>
      </Dialog>
      <ConfirmDialog
        open={isConfirmOpen}
        onOpenChange={setConfirmOpen}
        title="Delete this?"
        description="It disappears for everyone. This can't be undone."
        confirmLabel="Delete"
        onConfirm={async () => showToast("Deleted")}
      />
    </div>
  );
}

function Example({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section>
      <h2 className="mb-3 text-sm font-semibold text-muted">{title}</h2>
      <div className="flex flex-wrap items-center gap-3">{children}</div>
    </section>
  );
}

function GallerySelect({ id }: { id: string }) {
  const [start, setStart] = useState("540");
  return (
    <Select
      id={id}
      value={start}
      onChange={setStart}
      options={[
        { value: "540", label: "9:00 AM" },
        { value: "570", label: "9:30 AM" },
      ]}
    />
  );
}
