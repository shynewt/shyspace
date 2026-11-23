//! Typing in pages next to chat: how many ops page editing makes, and what a
//! newcomer's catch-up costs once a workspace's pages have been written.

use std::time::{Duration, Instant};

use yrs::{Doc, GetString, Text, TextRef, Transact, Update, updates::decoder::Decode};

use super::{
    counting_store::counting_bucket, database_bytes, median_time, megabytes, ms, peak_memory, per_second,
    scaled,
};
use crate::{Workspace, tests::support::*};

const PAGES: usize = 5;
/// About 70 words a minute: one keystroke every ~170 ms. The editor sends
/// what changed 120 ms after a change, so each keystroke is its own op.
const KEYSTROKES_PER_SECOND: f64 = 6.0;
/// The app uploads 600 ms after a write, so a typist's segment holds about
/// four keystrokes.
const KEYSTROKES_PER_SEGMENT: usize = 4;
/// A sentence or two at a time, then a pause.
const KEYSTROKES_PER_BURST: usize = 120;
/// Typos get fixed: every twentieth keystroke is a backspace.
const BACKSPACE_EVERY: usize = 20;
/// Chat between bursts, as in the catch-up probe.
const MESSAGES_PER_BURST: usize = 6;
const MESSAGES_PER_SEGMENT: usize = 3;
const CLOUD_REQUEST_TIME: Duration = Duration::from_millis(30);
const SENTENCE: &str = "The launch plan needs a budget review before the design goes out to hiring. ";

/// One member's open editor on a page, as the desktop keeps it.
struct Editor {
    doc: Doc,
    text: TextRef,
    nonce: String,
}

impl Editor {
    fn open(member: &Workspace, page: &str) -> Editor {
        let session = member.page_editor_session(page);
        let doc = Doc::with_client_id(session.client_id);
        let text = doc.get_or_insert_text("content");
        Editor {
            doc,
            text,
            nonce: session.nonce,
        }
    }

    /// Takes in what others wrote since, as the editor does live.
    fn catch_up(&self, member: &Workspace, page: &str) {
        if let Some(state) = member.page_state(page).unwrap() {
            self.doc
                .transact_mut()
                .apply_update(Update::decode_v1(&state).unwrap())
                .unwrap();
        }
    }

    fn len(&self) -> u32 {
        self.text.get_string(&self.doc.transact()).len() as u32
    }

    fn type_char(&self, at: u32, char: &str) -> Vec<u8> {
        let mut txn = self.doc.transact_mut();
        self.text.insert(&mut txn, at, char);
        txn.encode_update_v1()
    }

    fn backspace(&self, at: u32) -> Vec<u8> {
        let mut txn = self.doc.transact_mut();
        self.text.remove_range(&mut txn, at - 1, 1);
        txn.encode_update_v1()
    }
}

/// Types one burst into `page` at a spot that moves around the page, the way
/// people add to different paragraphs. Returns the ops it wrote.
async fn type_burst(member: &Workspace, editor: &Editor, page: &str, burst: usize) -> usize {
    editor.catch_up(member, page);
    let mut cursor = (burst as u32 * 7_919) % (editor.len() + 1);
    let mut typed = SENTENCE.chars().cycle();
    for keystroke in 1..=KEYSTROKES_PER_BURST {
        let update = if keystroke % BACKSPACE_EVERY == 0 && cursor > 0 {
            cursor -= 1;
            editor.backspace(cursor + 1)
        } else {
            let char = typed.next().expect("the sentence repeats").to_string();
            cursor += 1;
            editor.type_char(cursor - 1, &char)
        };
        member.apply_doc_update(page, &editor.nonce, update).unwrap();
        if keystroke % KEYSTROKES_PER_SEGMENT == 0 {
            member.sync().await;
        }
    }
    member.sync().await;
    KEYSTROKES_PER_BURST
}

#[tokio::test]
#[ignore = "capacity probe, see tests/capacity/mod.rs"]
async fn newcomer_catches_up_on_pages_and_chat() {
    let bursts = scaled(400);
    let root = tempfile::tempdir().unwrap();
    let (store, bucket) = counting_bucket();
    let ada = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&ada, &root.path().join("bob"), &bucket, "Bob", false).await;
    let channel = bob.create_channel("general").unwrap();
    let pages: Vec<String> = (0..PAGES)
        .map(|index| ada.create_page(&format!("Plan {index}"), None).unwrap())
        .collect();
    sync_all(&[&ada, &bob]).await;
    let typists = [&ada, &bob];
    let editors: Vec<[Editor; 2]> = pages
        .iter()
        .map(|page| [Editor::open(&ada, page), Editor::open(&bob, page)])
        .collect();

    let mut page_ops = 0;
    let mut messages = 0;
    let mut typing_time = Duration::ZERO;
    for burst in 0..bursts {
        let page = burst % PAGES;
        let typist = burst / PAGES % 2;
        let start = Instant::now();
        page_ops += type_burst(typists[typist], &editors[page][typist], &pages[page], burst).await;
        typing_time += start.elapsed();
        for index in 0..MESSAGES_PER_BURST {
            bob.send_message(
                &channel,
                &format!("Message {messages} about the launch plan"),
                None,
            )
            .unwrap();
            messages += 1;
            if index % MESSAGES_PER_SEGMENT == MESSAGES_PER_SEGMENT - 1 {
                bob.sync().await;
            }
        }
        sync_all(&[&ada, &bob]).await;
    }
    assert_eq!(ada.status().error, None);
    assert_eq!(bob.status().error, None);
    let texts: Vec<String> = pages.iter().map(|page| ada.page_text(page).unwrap()).collect();
    let typed_chars: usize = texts.iter().map(String::len).sum();
    assert_eq!(
        typed_chars,
        page_ops - page_ops / BACKSPACE_EVERY * 2,
        "every keystroke landed"
    );

    let before_join = store.requests();
    let carol = join(&ada, &root.path().join("carol"), &bucket, "Carol", false).await;
    let before_sync = store.requests();
    let sync_start = Instant::now();
    carol.sync().await;
    let sync_time = sync_start.elapsed();
    let catch_up = store.requests().since(&before_sync);
    let join_requests = before_sync.since(&before_join);
    assert_eq!(carol.status().error, None);
    for (page, text) in pages.iter().zip(&texts) {
        assert_eq!(&carol.page_text(page).unwrap(), text, "Carol sees the same page");
    }

    let dave = join(&ada, &root.path().join("dave"), &bucket, "Dave", false).await;
    store.set_delay(CLOUD_REQUEST_TIME);
    let cloud_start = Instant::now();
    dave.sync().await;
    let cloud_time = cloud_start.elapsed();
    store.set_delay(Duration::ZERO);
    assert_eq!(dave.status().error, None);

    let ops = page_ops + messages;
    let page_bytes: usize = pages
        .iter()
        .map(|page| carol.page_state(page).unwrap().map_or(0, |state| state.len()))
        .sum();
    let typing_seconds = page_ops as f64 / KEYSTROKES_PER_SECOND;
    println!("\n== A newcomer catches up on {bursts} typing bursts in {PAGES} pages, with chat ==");
    println!(
        "Written:                         {page_ops} page ops ({typed_chars} characters kept, about {:.1} h of typing) and {messages} messages",
        typing_seconds / 3600.0
    );
    println!(
        "Page ops per message:            {:.0}",
        page_ops as f64 / messages as f64
    );
    println!(
        "Typists apply their own keystrokes: {} (with a sync pass every {KEYSTROKES_PER_SEGMENT})",
        per_second(page_ops, typing_time)
    );
    println!(
        "Page state, all {PAGES} pages:        {}",
        megabytes(page_bytes as u64)
    );
    println!("Join, with the admin's passes:   {join_requests}");
    println!("First sync, in-memory bucket:    {}", ms(sync_time));
    println!("  applied:                       {}", per_second(ops, sync_time));
    println!("  bucket requests:               {catch_up}");
    println!(
        "First sync, {} a request:      {:.1} min (another newcomer)",
        ms(CLOUD_REQUEST_TIME),
        cloud_time.as_secs_f64() / 60.0
    );
    println!(
        "Carol's database:                {}",
        megabytes(database_bytes(&root.path().join("carol")))
    );
    println!(
        "Open a page:                     {}",
        ms(median_time(|| carol.page_text(&pages[0]).unwrap()))
    );
    println!("Peak memory of the test:         {}", peak_memory());
}
