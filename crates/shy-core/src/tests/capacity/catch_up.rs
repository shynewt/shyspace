//! A new member joining a workspace with a long history: how many bucket
//! requests and how long their first sync takes.

use std::time::{Duration, Instant};

use super::{counting_store::counting_bucket, median_time, ms, peak_memory, per_second, scaled};
use crate::tests::support::*;

/// People write in bursts: each sync pass after a burst uploads one segment.
const MESSAGES_PER_SEGMENT: usize = 3;
/// A typical time for one request to a cloud bucket over the internet.
const CLOUD_REQUEST_TIME: Duration = Duration::from_millis(30);

#[tokio::test]
#[ignore = "capacity probe, see tests/capacity/mod.rs"]
async fn newcomer_catches_up_on_history() {
    let messages = scaled(30_000);
    let root = tempfile::tempdir().unwrap();
    let (store, bucket) = counting_bucket();
    let ada = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&ada, &root.path().join("bob"), &bucket, "Bob", false).await;
    let channel = bob.create_channel("general").unwrap();
    for index in 0..messages {
        bob.send_message(&channel, &format!("Message {index} about the launch plan"), None)
            .unwrap();
        if index % MESSAGES_PER_SEGMENT == MESSAGES_PER_SEGMENT - 1 {
            bob.sync().await;
        }
    }
    bob.sync().await;
    ada.sync().await;
    assert_eq!(bob.status().error, None);

    let before_join = store.requests();
    let join_start = Instant::now();
    let carol = join(&ada, &root.path().join("carol"), &bucket, "Carol", false).await;
    let join_time = join_start.elapsed();
    let join_requests = store.requests().since(&before_join);
    let before_sync = store.requests();
    let sync_start = Instant::now();
    carol.sync().await;
    let sync_time = sync_start.elapsed();
    let catch_up = store.requests().since(&before_sync);
    assert_eq!(carol.status().error, None);
    let latest = carol
        .latest_message(&channel)
        .unwrap()
        .expect("Carol sees the history");
    assert_eq!(
        latest.message.text,
        format!("Message {} about the launch plan", messages - 1)
    );

    let before_idle = store.requests();
    carol.sync().await;
    let idle_pass = store.requests().since(&before_idle);

    let dave = join(&ada, &root.path().join("dave"), &bucket, "Dave", false).await;
    store.set_delay(CLOUD_REQUEST_TIME);
    let cloud_sync_start = Instant::now();
    dave.sync().await;
    let cloud_sync_time = cloud_sync_start.elapsed();
    store.set_delay(Duration::ZERO);
    assert_eq!(dave.status().error, None);

    println!("\n== A newcomer catches up on {messages} messages ==");
    println!(
        "Join, with the admin's passes:   {} ({join_requests})",
        ms(join_time)
    );
    println!("First sync, in-memory bucket:    {}", ms(sync_time));
    println!(
        "  applied:                       {}",
        per_second(messages, sync_time)
    );
    println!("  bucket requests:               {catch_up}");
    println!(
        "First sync, {} a request:      {:.1} min (another newcomer)",
        ms(CLOUD_REQUEST_TIME),
        cloud_sync_time.as_secs_f64() / 60.0
    );
    println!("Next, idle sync pass:            {idle_pass}");
    println!(
        "First sidebar (all unread):      {}",
        ms(median_time(|| carol.channels().unwrap()))
    );
    println!("Peak memory of the test:         {}", peak_memory());
}
