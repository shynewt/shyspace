import { Bell, Plus, Settings, Trash2 } from "lucide-react";
import { type ReactNode, useState } from "react";
import { showToast } from "../lib/toast";
import {
  Avatar,
  Badge,
  Button,
  Card,
  ConfirmDialog,
  CopyField,
  Dialog,
  EmojiPicker,
  EmptyState,
  Field,
  FormError,
  IconButton,
  Input,
  Kbd,
  Logo,
  Menu,
  Popover,
  SectionLabel,
  SegmentedControl,
  Spinner,
  TextArea,
  Tooltip,
} from "../ui";

/** Every design-system component in one place. Open the app with #gallery in development. */
export default function Gallery() {
  const [isDialogOpen, setDialogOpen] = useState(false);
  const [isConfirmOpen, setConfirmOpen] = useState(false);
  const [segment, setSegment] = useState<"one" | "two" | "three">("one");
  return (
    <div className="h-full overflow-y-auto bg-surface">
      <div className="mx-auto flex max-w-[860px] flex-col gap-8 px-8 py-10">
        <header className="flex items-center gap-3">
          <Logo size={32} />
          <h1 className="text-[22px] font-semibold tracking-tight">shyspace design system</h1>
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
            <FormError message="The bucket refused these keys: 403 Forbidden" />
          </div>
        </Example>

        <Example title="Selection">
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
        </Example>

        <Example title="Floating surfaces">
          <Button onClick={() => setDialogOpen(true)}>Open dialog</Button>
          <Button onClick={() => setConfirmOpen(true)}>Confirm a delete</Button>
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
            <div className="p-3 text-[13px]">Anchored to its trigger.</div>
          </Popover>
          <Popover trigger={<Button>Pick an emoji</Button>} side="bottom" align="start">
            <EmojiPicker onSelect={(emoji) => showToast(`Picked ${emoji}`)} />
          </Popover>
          <Button onClick={() => showToast("Saved")}>Show toast</Button>
        </Example>

        <Example title="Content">
          <Avatar id="a" name="Ada Lovelace" />
          <Avatar id="b" name="Claude" isBot />
          <Badge tone="accent">bot</Badge>
          <Badge>neutral</Badge>
          <Badge tone="warning">warning</Badge>
          <Badge tone="danger">danger</Badge>
          <Badge tone="success">success</Badge>
          <Kbd>Ctrl K</Kbd>
          <Spinner />
        </Example>

        <Example title="Cards and copy fields">
          <div className="grid w-full grid-cols-2 gap-3">
            <Card>
              <SectionLabel>Plain card</SectionLabel>
              <CopyField value="shy2_example-invite-code" />
            </Card>
            <Card highlighted>
              <SectionLabel>Highlighted card</SectionLabel>
              <p className="text-[13px] text-muted">For things that need attention, like join requests.</p>
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
        <div className="flex justify-end gap-2 p-5">
          <Button variant="ghost" onClick={() => setDialogOpen(false)}>
            Cancel
          </Button>
          <Button variant="primary" onClick={() => setDialogOpen(false)}>
            Confirm
          </Button>
        </div>
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
      <h2 className="mb-3 text-[13px] font-semibold text-muted">{title}</h2>
      <div className="flex flex-wrap items-center gap-3">{children}</div>
    </section>
  );
}
