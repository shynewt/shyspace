//! Years of messages: how writing, syncing and the everyday screens slow
//! down as one workspace's history grows.

use std::time::Instant;

use super::{
    counting_store::counting_bucket, database_bytes, median_time, megabytes, ms, peak_memory, per_second,
    scaled, timed,
};
use crate::tests::support::*;

const CHANNELS: usize = 10;
/// Every fourth message answers a recent one, in a thread.
const REPLY_EVERY: usize = 4;
const MENTION_EVERY: usize = 100;
const REACT_EVERY: usize = 10;
const SEARCHED_WORD: &str = "budget";
const PAGE: u32 = 50;
const WORDS: [&str; 8] = [
    "launch", "budget", "design", "hiring", "roadmap", "invoice", "review", "deploy",
];

/// What one member wrote since the last measurement.
#[derive(Default)]
struct Written {
    in_channel: usize,
    mentions: usize,
}

#[tokio::test]
#[ignore = "capacity probe, see tests/capacity/mod.rs"]
async fn screens_as_history_grows() {
    let steps = [scaled(10_000), scaled(30_000), scaled(100_000)];
    let root = tempfile::tempdir().unwrap();
    let (store, bucket) = counting_bucket();
    let ada = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&ada, &root.path().join("bob"), &bucket, "Bob", false).await;
    let channels: Vec<String> = (0..CHANNELS)
        .map(|index| bob.create_channel(&format!("team-{index}")).unwrap())
        .collect();
    sync_all(&[&ada, &bob]).await;
    let mention = format!("[@Ada](shy://member/{})", ada.my_id());
    let mut latest_root: Vec<Option<String>> = vec![None; CHANNELS];
    let mut thread_root = None;
    let mut shown_in_opened = 0;
    let mut written = 0;
    println!("\n== History: Bob writes, Ada (never online in between) syncs and reads ==");
    for step in steps {
        let mut since = Written::default();
        let (_, write_time) = timed(|| {
            for index in written..step {
                let channel = &channels[index % CHANNELS];
                let is_mention = index % MENTION_EVERY == 0;
                let text = format!(
                    "{}Update {index}: the {} plan looks good, notes are in the doc",
                    if is_mention {
                        format!("{mention} ")
                    } else {
                        String::new()
                    },
                    WORDS[index % WORDS.len()],
                );
                let reply_to = latest_root[index % CHANNELS]
                    .clone()
                    .filter(|_| index % REPLY_EVERY == 0);
                let id = bob.send_message(channel, &text, reply_to.clone()).unwrap();
                if index % REACT_EVERY == 0 {
                    bob.react(&id, "👍", true).unwrap();
                }
                match reply_to {
                    Some(root) => {
                        thread_root.get_or_insert(root);
                    }
                    None => {
                        since.in_channel += 1;
                        shown_in_opened += usize::from(index % CHANNELS == 0);
                        latest_root[index % CHANNELS] = Some(id);
                    }
                }
                since.mentions += usize::from(is_mention);
            }
        });
        let new_messages = step - written;
        written = step;
        bob.sync().await;
        let start = Instant::now();
        ada.sync().await;
        let ingest_time = start.elapsed();
        assert_eq!(ada.status().error, None);

        let sidebar = ada.channels().unwrap();
        let unread: u32 = sidebar.iter().map(|channel| channel.unread).sum();
        let unread_mentions: u32 = sidebar.iter().map(|channel| channel.unread_mentions).sum();
        assert_eq!(
            unread as usize, since.in_channel,
            "unread count after catching up"
        );
        assert_eq!(
            unread_mentions as usize, since.mentions,
            "unread mentions after catching up"
        );
        let opened = &channels[0];
        let root_id = thread_root.clone().expect("some messages are replies");

        println!("\n-- {step} messages ({new_messages} new, {unread} unread in channels for Ada) --");
        println!(
            "Bob writes:                   {}",
            per_second(new_messages, write_time)
        );
        println!(
            "Ada applies from the bucket:  {} ({})",
            per_second(new_messages, ingest_time),
            ms(ingest_time)
        );
        println!(
            "Ada's database:               {} ({} bytes per message)",
            megabytes(database_bytes(&root.path().join("ada"))),
            database_bytes(&root.path().join("ada")) / step as u64
        );
        println!("Bucket so far:                {}", store.requests());
        println!("With unread messages:");
        println!(
            "  sidebar (channels):         {}",
            ms(median_time(|| ada.channels().unwrap()))
        );
        println!(
            "  inbox:                      {}",
            ms(median_time(|| ada.inbox().unwrap()))
        );
        println!(
            "  open a channel ({PAGE} msgs):   {}",
            ms(median_time(|| ada.messages(opened, None, PAGE).unwrap()))
        );
        let (_, mark_time) = timed(|| ada.mark_all_read().unwrap());
        println!("  mark all read:              {}", ms(mark_time));
        println!("Caught up:");
        println!(
            "  sidebar (channels):         {}",
            ms(median_time(|| ada.channels().unwrap()))
        );
        println!(
            "  inbox:                      {}",
            ms(median_time(|| ada.inbox().unwrap()))
        );
        println!(
            "  open a channel ({PAGE} msgs):   {}",
            ms(median_time(|| ada.messages(opened, None, PAGE).unwrap()))
        );
        println!(
            "  open a thread:              {}",
            ms(median_time(|| ada.thread(&root_id).unwrap()))
        );
        println!(
            "  search \"{SEARCHED_WORD}\" (20 hits):   {}",
            ms(median_time(|| ada.search(SEARCHED_WORD, 20).unwrap()))
        );
        println!(
            "  Bob's sent list ({PAGE}):      {}",
            ms(median_time(|| bob.sent_messages("", None, PAGE).unwrap()))
        );
        assert_eq!(
            ada.messages(opened, None, PAGE).unwrap().len(),
            shown_in_opened.min(PAGE as usize)
        );
        assert!(ada.channels().unwrap().iter().all(|channel| channel.unread == 0));
        println!("Peak memory of the test:      {}", peak_memory());
    }
}
