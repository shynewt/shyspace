//! The workspace's icon and default look: the admin sets them, every member
//! sees them, and anyone can choose their own look on their device instead.

use anyhow::{Result, ensure};

use super::Workspace;
use crate::{
    ops::{Op, is_look_name},
    views::WorkspaceLook,
};

impl Workspace {
    /// Sets the workspace's icon (an already cropped PNG, JPEG or WebP),
    /// or goes back to its letter with `None`. Admin only.
    pub async fn set_workspace_icon(&self, image: Option<Vec<u8>>) -> Result<()> {
        self.ensure_may_set_look(&Op::WorkspaceIcon { picture: None })?;
        self.share_picture(image, |picture| Op::WorkspaceIcon { picture })
            .await
    }

    /// The icon's bytes, if the admin set one.
    pub async fn workspace_icon(&self) -> Result<Option<Vec<u8>>> {
        let picture = self.state().store.workspace_icon()?;
        self.picture_bytes(&self.inner.config.admin, picture).await
    }

    /// The icon's id, which changes whenever the icon does: for caching it.
    pub fn workspace_icon_id(&self) -> Result<Option<String>> {
        Ok(self.state().store.workspace_icon()?.map(|picture| picture.id))
    }

    /// Sets the theme and accent members see unless they chose their own;
    /// `None` leaves the app's default. Admin only.
    pub fn set_workspace_look(&self, theme: Option<&str>, accent: Option<&str>) -> Result<()> {
        for name in [theme, accent].into_iter().flatten() {
            ensure!(
                is_look_name(name),
                "a theme or accent is named by 1 to 24 lowercase letters"
            );
        }
        let op = Op::WorkspaceLook {
            theme: theme.map(str::to_owned),
            accent: accent.map(str::to_owned),
        };
        self.ensure_may_set_look(&op)?;
        self.write(op)
    }

    pub fn workspace_look(&self) -> Result<WorkspaceLook> {
        self.state().store.workspace_look()
    }

    fn ensure_may_set_look(&self, op: &Op) -> Result<()> {
        let may_set = {
            let store = &self.state().store;
            self.may(store, store.newest_epoch()?, op)?
        };
        ensure!(
            may_set,
            "only the workspace admin can change the workspace's icon and look"
        );
        Ok(())
    }
}
