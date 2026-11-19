import type { SectionSort, SidebarSection } from "../../../src/lib/api";
import { randomHex } from "./helpers";

const BUILTIN = "channels";

type Section = { id: string; name: string; sort: SectionSort; is_collapsed: boolean };

/** Sidebar sections, like the core keeps them: personal, built-in first. */
export class FakeSections {
  private sections: Section[] = [{ id: BUILTIN, name: "Channels", sort: "name", is_collapsed: false }];
  private placement = new Map<string, string>();

  create(name: string): string {
    const id = randomHex(8);
    this.sections.push({ id, name: name.trim(), sort: "name", is_collapsed: false });
    return id;
  }

  rename(id: string, name: string): void {
    this.custom(id).name = name.trim();
  }

  delete(id: string): void {
    this.custom(id);
    this.sections = this.sections.filter((section) => section.id !== id);
    for (const [channel, section] of this.placement) if (section === id) this.placement.delete(channel);
  }

  setSort(id: string, sort: SectionSort): void {
    this.find(id).sort = sort;
  }

  setCollapsed(id: string, isCollapsed: boolean): void {
    this.find(id).is_collapsed = isCollapsed;
  }

  move(channel: string, section: string | null): void {
    if (section && section !== BUILTIN) this.placement.set(channel, this.find(section).id);
    else this.placement.delete(channel);
  }

  list(channels: { id: string; name: string; lastActivity: number }[]): SidebarSection[] {
    return this.sections.map((section) => {
      const members = channels.filter((channel) => (this.placement.get(channel.id) ?? BUILTIN) === section.id);
      members.sort((left, right) =>
        section.sort === "recent"
          ? right.lastActivity - left.lastActivity || left.name.localeCompare(right.name)
          : left.name.localeCompare(right.name),
      );
      return { ...section, is_builtin: section.id === BUILTIN, channels: members.map((channel) => channel.id) };
    });
  }

  private find(id: string): Section {
    const section = this.sections.find((candidate) => candidate.id === id);
    if (!section) throw new Error(`fake backend: no section ${id}`);
    return section;
  }

  private custom(id: string): Section {
    if (id === BUILTIN) throw "the Channels section can't be renamed or deleted";
    return this.find(id);
  }
}
