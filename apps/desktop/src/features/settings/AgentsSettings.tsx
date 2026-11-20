import { useState } from "react";
import { api } from "../../lib/api";
import { useFormAction } from "../../lib/useFormAction";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { Button, CopyField, Field, FormError, Input } from "../../ui";
import { MemberRows } from "../members/MemberList";
import { RemoveMemberButton } from "../members/RemoveMember";
import { useAdminName } from "./layout";

export function AgentsSettings() {
  const { session } = useWorkspace();
  const members = useLive(api.members, (change) => change.kind === "members");
  const agents = members?.filter((member) => member.is_bot && member.active) ?? [];
  return (
    <div className="flex flex-col gap-5">
      <p className="text-sm leading-relaxed">
        Agents, like Claude Code, join as members, so they can read every channel and page. What they read goes to their
        AI provider: end-to-end encryption ends there.
      </p>
      {members &&
        (agents.length > 0 ? (
          <MemberRows
            label="Agents"
            members={agents}
            actions={(agent) => session.is_admin && <RemoveMemberButton member={agent} />}
          />
        ) : (
          <p className="text-sm text-muted">No agents yet.</p>
        ))}
      {session.is_admin ? <AddAgent /> : <AskAdmin />}
    </div>
  );
}

function AskAdmin() {
  const adminName = useAdminName();
  if (!adminName) return null;
  return <p className="text-sm text-muted">Ask {adminName} to add an agent.</p>;
}

function AddAgent() {
  const { openSettings } = useWorkspace();
  const [name, setName] = useState("Claude");
  const [invite, setInvite] = useState<{ code: string; agentName: string } | null>(null);
  const create = useFormAction(async () => {
    const agentName = name.trim();
    setInvite({ code: await api.createInvite(agentName), agentName });
  });
  return (
    <section aria-labelledby="add-agent" className="flex flex-col gap-3 border-t border-line pt-4">
      <h4 id="add-agent" className="text-sm font-semibold">
        Add an agent
      </h4>
      <form onSubmit={create.run} className="flex items-end gap-2">
        <div className="flex-1">
          <Field label="Agent name">
            {(id) => <Input id={id} value={name} onChange={(event) => setName(event.target.value)} />}
          </Field>
        </div>
        <Button type="submit" variant="primary" isLoading={create.isBusy} disabled={!name.trim()} className="h-9">
          Create agent invite
        </Button>
      </form>
      <FormError message={create.error} />
      {invite ? (
        <>
          <p className="text-xs leading-relaxed text-muted">
            Run these where {invite.agentName} works, then let it in under{" "}
            <button
              type="button"
              onClick={() => openSettings({ section: "members", membersTab: "requests" })}
              className="font-medium text-accent-text underline-offset-2 hover:underline"
            >
              Members › Requests
            </button>
            .
          </p>
          <CopyField value={agentCommands(invite.agentName, invite.code)} label="Copy commands" />
        </>
      ) : (
        <p className="text-xs leading-relaxed text-muted">
          The invite comes with the commands to run where the agent works, like Claude Code with the shy CLI installed.
        </p>
      )}
    </section>
  );
}

function agentCommands(name: string, inviteCode: string): string {
  return `shy join --bot --name ${shellQuote(name)} ${inviteCode}\nclaude mcp add shyspace -- shy mcp`;
}

/** Single-quotes text for a POSIX shell, so a name can't run anything when pasted. */
function shellQuote(text: string): string {
  return `'${text.replaceAll("'", `'\\''`)}'`;
}
