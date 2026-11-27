//! Who else has a page open and where their cursors are, live and never
//! stored: opening a page joins its own gossip topic (see
//! `transport::page_gossip`), sharing repeats a signed beacon while it stays
//! open, and what others send is checked before it shows.

use anyhow::{Context, Result, ensure};
use iroh::PublicKey;
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{debug, warn};

use super::Workspace;
use crate::{
    ops::{EntityId, now_ms},
    pages::cursors::{BEACON_EVERY_MS, CursorSelection, CursorUpdate, Cursors, PageBeacon, PageViewers},
    transport::page_gossip::{PageWire, page_topic},
    views::{Change, PageCursor, PagePresence, PresenceState},
};

/// Page messages waiting to be checked; more are dropped, as the next cursor
/// update replaces a lost one.
const PAGE_INBOX: usize = 64;
/// Newcomers are greeted at most this often, however many arrive.
const MIN_GREETING_GAP_MS: i64 = 1000;

/// The page open on this device, and what's been heard on it.
#[derive(Default)]
pub(crate) struct PageLive {
    open: Option<OpenPage>,
    viewers: PageViewers,
    cursors: Cursors,
}

struct OpenPage {
    page: EntityId,
    /// Whether others hear that we're here and where our cursor is.
    is_shared: bool,
    /// Our last selection, told again to whoever opens the page after.
    selection: Option<CursorSelection>,
    last_greeted_ms: i64,
    /// Beacons, and taking what arrives on the page's topic.
    task: JoinHandle<()>,
}

impl Drop for OpenPage {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Workspace {
    /// Starts hearing who is on `page` and where their cursors are, leaving
    /// any other page; with `share`, they hear about us too.
    pub async fn join_page(&self, page: &str, share: bool) -> Result<()> {
        ensure!(self.pages()?.iter().any(|known| known.id == page), "no such page");
        let (was_shared, is_open_already) = {
            let live = self.page_live();
            let open = live.open.as_ref();
            (
                open.is_some_and(|open| open.is_shared),
                open.is_some_and(|open| open.page == page),
            )
        };
        if is_open_already {
            return self.set_sharing(page, share, was_shared).await;
        }
        if was_shared && let Some(left) = self.open_page_id() {
            self.say_left(&left).await;
        }
        let (deliver, messages) = mpsc::channel(PAGE_INBOX);
        let task = self
            .inner
            .runtime
            .spawn(self.clone().run_page(page.to_owned(), deliver, messages));
        *self.page_live() = PageLive {
            open: Some(OpenPage {
                page: page.to_owned(),
                is_shared: share,
                selection: None,
                last_greeted_ms: 0,
                task,
            }),
            ..PageLive::default()
        };
        self.emit(Change::PagePresence {
            page: page.to_owned(),
        });
        Ok(())
    }

    /// Turns sharing on the open page on or off, telling the others at once.
    async fn set_sharing(&self, page: &str, share: bool, was_shared: bool) -> Result<()> {
        if let Some(open) = self.page_live().open.as_mut() {
            open.is_shared = share;
        }
        match (was_shared, share) {
            (false, true) => {
                self.broadcast_page(PageWire::Beacon(self.page_beacon(page, true)?))
                    .await
            }
            (true, false) => {
                self.say_left(page).await;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn open_page_id(&self) -> Option<EntityId> {
        self.page_live().open.as_ref().map(|open| open.page.clone())
    }

    fn is_sharing(&self, page: &str) -> bool {
        self.page_live()
            .open
            .as_ref()
            .is_some_and(|open| open.page == page && open.is_shared)
    }

    /// Stops hearing `page`, saying so to the others if we shared it.
    pub async fn leave_page(&self, page: &str) -> Result<()> {
        let was_shared = {
            let mut live = self.page_live();
            let Some(open) = live.open.as_ref().filter(|open| open.page == page) else {
                return Ok(());
            };
            let was_shared = open.is_shared;
            *live = PageLive::default();
            was_shared
        };
        if was_shared {
            self.say_left(page).await;
        }
        if let Some(net) = &self.inner.net {
            net.leave_page();
        }
        Ok(())
    }

    /// Forgets the open page without telling anyone, as the workspace closes.
    pub(crate) fn close_page(&self) {
        *self.page_live() = PageLive::default();
        if let Some(net) = &self.inner.net {
            net.leave_page();
        }
    }

    /// Tells the others on `page` where our selection is (none: not in its text).
    pub async fn move_cursor(&self, page: &str, selection: Option<CursorSelection>) -> Result<()> {
        {
            let mut live = self.page_live();
            let open = live
                .open
                .as_mut()
                .filter(|open| open.page == page && open.is_shared)
                .context("not sharing a cursor on that page")?;
            open.selection = selection.clone();
        }
        self.send_cursor(page, selection).await
    }

    /// Who else is on `page` and their cursors; nobody unless it's the page
    /// we joined. Others are named by person, once each; my own other
    /// devices aren't "someone else".
    pub fn page_presence(&self, page: &str) -> Result<PagePresence> {
        let members = self.current_members()?;
        let now = now_ms()?;
        let (viewers, heard) = {
            let live = self.page_live();
            if live.open.as_ref().is_none_or(|open| open.page != page) {
                return Ok(PagePresence::default());
            }
            // Read back through current members: someone removed since is gone at once.
            let viewers: Vec<PublicKey> = live
                .viewers
                .viewers(page, now)
                .into_iter()
                .filter(|viewer| members.contains(viewer) && *viewer != self.me())
                .collect();
            let heard = live.cursors.on_page(page, &viewers);
            (viewers, heard)
        };
        let state = self.state();
        let me = self.my_person();
        let mut people: Vec<String> = Vec::new();
        for viewer in &viewers {
            let person = state.store.person_of(viewer)?;
            let id = hex::encode(person.as_bytes());
            if person != me && !people.contains(&id) {
                people.push(id);
            }
        }
        let mut cursors = Vec::new();
        for (device, heard) in heard {
            let person = state.store.person_of(&device)?;
            if person != me {
                cursors.push(PageCursor {
                    member: hex::encode(person.as_bytes()),
                    selection: heard.selection,
                    idle_ms: now - heard.received_ms,
                });
            }
        }
        Ok(PagePresence {
            viewers: people,
            cursors,
        })
    }

    /// Takes a message from the open page's topic; anything for another
    /// page, or that fails its checks, is dropped.
    pub(crate) fn receive_page(&self, message: &PageWire) -> Result<()> {
        let members = self.current_members()?;
        let is_member = |member: &PublicKey| members.contains(member);
        let workspace_id = &self.inner.config.workspace_id;
        let now = now_ms()?;
        let page = match message {
            PageWire::Beacon(beacon) => &beacon.page,
            PageWire::Cursor(update) => &update.page,
        };
        let (changed, greeting) = {
            let mut live = self.page_live();
            let PageLive {
                open,
                viewers,
                cursors,
            } = &mut *live;
            let Some(open) = open.as_mut().filter(|open| open.page == *page) else {
                return Ok(());
            };
            match message {
                PageWire::Beacon(beacon) => {
                    let changed = viewers.accept(beacon, workspace_id, is_member, now)?;
                    let should_greet = changed
                        && beacon.is_open
                        && open.is_shared
                        && now - open.last_greeted_ms >= MIN_GREETING_GAP_MS;
                    if should_greet {
                        open.last_greeted_ms = now;
                    }
                    (changed, should_greet.then(|| open.selection.clone()))
                }
                PageWire::Cursor(update) => (cursors.accept(update, workspace_id, is_member, now)?, None),
            }
        };
        if let Some(selection) = greeting {
            self.inner
                .runtime
                .spawn(self.clone().greet_on_page(page.clone(), selection));
        }
        if changed {
            self.emit(Change::PagePresence { page: page.clone() });
        }
        Ok(())
    }

    /// Someone just opened the page: tells them at once that we're on it and
    /// where our cursor is, rather than at our next beacon or move.
    async fn greet_on_page(self, page: String, selection: Option<CursorSelection>) {
        let greeted = async {
            self.broadcast_page(PageWire::Beacon(self.page_beacon(&page, true)?))
                .await?;
            self.send_cursor(&page, selection).await
        };
        if let Err(error) = greeted.await {
            warn!("couldn't greet someone on a page: {error:#}");
        }
    }

    async fn say_left(&self, page: &str) {
        let said = async {
            self.broadcast_page(PageWire::Beacon(self.page_beacon(page, false)?))
                .await
        };
        if let Err(error) = said.await {
            warn!("couldn't say we left a page: {error:#}");
        }
    }

    async fn send_cursor(&self, page: &str, selection: Option<CursorSelection>) -> Result<()> {
        let update = CursorUpdate::signed(
            &self.inner.identity,
            &self.inner.config.workspace_id,
            page,
            selection,
            now_ms()?,
        )?;
        self.broadcast_page(PageWire::Cursor(update)).await
    }

    async fn broadcast_page(&self, message: PageWire) -> Result<()> {
        #[cfg(test)]
        self.inner
            .page_sent
            .lock()
            .expect("sent page messages lock poisoned by an earlier panic")
            .push(message.clone());
        if let Some(net) = &self.inner.net {
            net.broadcast_page(&message).await?;
        }
        Ok(())
    }

    /// While `page` stays open, and until the workspace stops: takes what
    /// arrives on its topic, and every so often joins it with whoever is
    /// around (members come and go) and, with `share`, says we're on it.
    async fn run_page(
        self,
        page: String,
        deliver: mpsc::Sender<PageWire>,
        mut messages: mpsc::Receiver<PageWire>,
    ) {
        let mut stopped = self.inner.stopped.subscribe();
        if *stopped.borrow() {
            return;
        }
        let mut tick =
            tokio::time::interval(std::time::Duration::from_millis(BEACON_EVERY_MS.unsigned_abs()));
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    if let Err(error) = self.announce_on_page(&page, &deliver).await {
                        warn!("couldn't reach the others on a page: {error:#}");
                    }
                    // Someone not heard from lately has left by now, and nothing else says so.
                    self.emit(Change::PagePresence { page: page.clone() });
                }
                Some(message) = messages.recv() => {
                    if let Err(error) = self.receive_page(&message) {
                        debug!("ignoring a page message: {error:#}");
                    }
                }
                _ = stopped.changed() => break,
            }
        }
    }

    async fn announce_on_page(&self, page: &str, deliver: &mpsc::Sender<PageWire>) -> Result<()> {
        let Some(net) = &self.inner.net else { return Ok(()) };
        let now = now_ms()?;
        let around: Vec<PublicKey> = self
            .current_members()?
            .into_iter()
            .filter(|member| self.presence_of(member, now) != PresenceState::Unreachable)
            .collect();
        net.join_page(
            page_topic(&self.inner.config.workspace_id, page)?,
            around,
            deliver.clone(),
        )
        .await?;
        if self.is_sharing(page) {
            self.broadcast_page(PageWire::Beacon(self.page_beacon(page, true)?))
                .await?;
        }
        Ok(())
    }

    fn page_beacon(&self, page: &str, is_open: bool) -> Result<PageBeacon> {
        PageBeacon::signed(
            &self.inner.identity,
            &self.inner.config.workspace_id,
            page,
            is_open,
            now_ms()?,
        )
    }

    fn page_live(&self) -> std::sync::MutexGuard<'_, PageLive> {
        self.inner
            .page_live
            .lock()
            .expect("page live lock poisoned by an earlier panic")
    }
}
