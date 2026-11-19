//! Sidebar sections: personal groups of channels.

use anyhow::{Result, bail};

use super::Workspace;
use crate::{
    ops::new_random_id,
    storage::{BUILTIN_SECTION, ItemKind},
    views::{Change, SectionSort, SidebarSection},
};

impl Workspace {
    /// Sections in order, each with its channels sorted as it asks.
    pub fn sidebar_sections(&self) -> Result<Vec<SidebarSection>> {
        let channels = self.channels()?;
        let state = self.state();
        let mut sections = Vec::new();
        for row in state.store.sections()? {
            let mut members = Vec::new();
            for channel in &channels {
                let section = state.store.placement(&channel.id)?;
                if section.as_deref().unwrap_or(BUILTIN_SECTION) == row.id {
                    members.push((channel, state.store.last_activity_ms(&channel.id)?));
                }
            }
            match row.sort {
                SectionSort::Name => members.sort_by(|left, right| left.0.name.cmp(&right.0.name)),
                SectionSort::Recent => {
                    members.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.name.cmp(&right.0.name)))
                }
            }
            sections.push(SidebarSection {
                is_builtin: row.id == BUILTIN_SECTION,
                id: row.id,
                name: row.name,
                sort: row.sort,
                is_collapsed: row.is_collapsed,
                channels: members
                    .into_iter()
                    .map(|(channel, _)| channel.id.clone())
                    .collect(),
            });
        }
        Ok(sections)
    }

    pub fn create_section(&self, name: &str) -> Result<String> {
        let name = name.trim();
        if name.is_empty() {
            bail!("a section needs a name");
        }
        let id = new_random_id();
        self.state().store.create_section(&id, name)?;
        self.emit(Change::Channels);
        Ok(id)
    }

    pub fn rename_section(&self, section: &str, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            bail!("a section needs a name");
        }
        self.state().store.rename_section(section, name)?;
        self.emit(Change::Channels);
        Ok(())
    }

    /// Its channels go back to "Channels".
    pub fn delete_section(&self, section: &str) -> Result<()> {
        self.state().store.delete_section(section)?;
        self.emit(Change::Channels);
        Ok(())
    }

    pub fn set_section_sort(&self, section: &str, sort: SectionSort) -> Result<()> {
        self.state().store.set_section_sort(section, sort)?;
        self.emit(Change::Channels);
        Ok(())
    }

    pub fn set_section_collapsed(&self, section: &str, is_collapsed: bool) -> Result<()> {
        self.state().store.set_section_collapsed(section, is_collapsed)?;
        self.emit(Change::Channels);
        Ok(())
    }

    /// Moves `channel` into `section`, or back to "Channels" with `None`.
    pub fn move_channel(&self, channel: &str, section: Option<&str>) -> Result<()> {
        if self.state().store.item_kind(channel)? != Some(ItemKind::Channel) {
            bail!("no such channel");
        }
        self.state().store.place_channel(channel, section)?;
        self.emit(Change::Channels);
        Ok(())
    }
}
