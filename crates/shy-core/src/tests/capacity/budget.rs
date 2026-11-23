//! What sync costs in bucket requests per person and hour (#137), measured
//! with the counting bucket: each kind of event is measured once, then
//! multiplied by how often it happens. Assumptions, as in the projection:
//! checks settle at one a minute when idle, the 10-minute duties run six
//! times an hour, a sweep once; a busy hour has 150 uploads a workday from
//! each member (about 19 an hour); a month is 190 hours online, 168 busy.

use super::{
    counting_store::{CountingStore, Requests, counting_bucket},
    scaled,
};
use crate::tests::support::*;

const CHECKS_PER_HOUR: u64 = 60;
const SLOW_DUTIES_PER_HOUR: u64 = 6;
const UPLOADS_PER_MEMBER_HOUR: u64 = 19;
/// Checks an hour when gossip is down and news keeps the timer at 8 s.
const CHECKS_PER_BUSY_HOUR_WITHOUT_GOSSIP: u64 = 450;
const BUSY_HOURS: u64 = 168;
const IDLE_HOURS: u64 = 190 - BUSY_HOURS;
/// S3 prices per 1,000 requests: LIST and PUT, then GET.
const LIST_PUT_PRICE: f64 = 0.005;
const GET_PRICE: f64 = 0.0004;

#[derive(Clone, Copy, Default)]
struct Cost {
    list_pages: u64,
    puts: u64,
    gets: u64,
}

impl Cost {
    fn of(requests: Requests) -> Self {
        Self {
            list_pages: requests.list_pages(),
            puts: requests.puts,
            gets: requests.gets,
        }
    }

    fn times(self, count: u64) -> Self {
        Self {
            list_pages: self.list_pages * count,
            puts: self.puts * count,
            gets: self.gets * count,
        }
    }

    fn plus(self, other: Self) -> Self {
        Self {
            list_pages: self.list_pages + other.list_pages,
            puts: self.puts + other.puts,
            gets: self.gets + other.gets,
        }
    }

    fn minus(self, other: Self) -> Self {
        Self {
            list_pages: self.list_pages.saturating_sub(other.list_pages),
            puts: self.puts.saturating_sub(other.puts),
            gets: self.gets.saturating_sub(other.gets),
        }
    }

    fn dollars(self) -> f64 {
        (self.list_pages + self.puts) as f64 / 1000.0 * LIST_PUT_PRICE + self.gets as f64 / 1000.0 * GET_PRICE
    }
}

impl std::fmt::Display for Cost {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} lists, {} puts, {} gets",
            self.list_pages, self.puts, self.gets
        )
    }
}

async fn measured<T>(store: &CountingStore, work: impl std::future::Future<Output = T>) -> Cost {
    let before = store.requests();
    work.await;
    Cost::of(store.requests().since(&before))
}

async fn budget_at(member_count: usize) {
    let root = tempfile::tempdir().unwrap();
    let (store, bucket) = counting_bucket();
    let admin = create_admin(&root.path().join("admin"), &bucket).await;
    let mut members = Vec::new();
    for number in 1..member_count {
        let name = format!("member{number}");
        members.push(join(&admin, &root.path().join(&name), &bucket, &name, false).await);
    }
    let (reader, writer) = (&members[0], &members[1]);
    let general = channel_id(&admin, "general");
    for member in std::iter::once(&admin).chain(members.iter()) {
        member.sync().await;
    }
    for _ in 0..3 {
        reader.check().await;
    }

    let idle = measured(&store, reader.check()).await;
    reader.make_slow_duties_due();
    let slow = measured(&store, reader.check()).await.minus(idle);
    let sweep = measured(&store, reader.sync()).await;

    writer.send_message(&general, "hello", None).unwrap();
    for envelope in writer.state().store.unpushed(&writer.me()).unwrap() {
        reader.receive(envelope).unwrap();
    }
    let upload = measured(&store, async { writer.push().await.unwrap() }).await;
    let seq = writer.state().store.last_seq(&writer.me()).unwrap();
    let head_version = bucket
        .heads()
        .await
        .unwrap()
        .into_iter()
        .find(|head| head.author == writer.me())
        .unwrap()
        .version;
    reader
        .receive_hint(&writer.signed_hint(seq, head_version).unwrap())
        .unwrap();
    let hinted = measured(&store, async {
        reader.check().await;
    })
    .await
    .minus(idle);

    writer.send_message(&general, "gossip lost this", None).unwrap();
    writer.push().await.unwrap();
    let unhinted = measured(&store, async {
        reader.check().await;
    })
    .await
    .minus(idle);

    let others_uploads = UPLOADS_PER_MEMBER_HOUR * (member_count as u64 - 1);
    let idle_hour = idle
        .times(CHECKS_PER_HOUR)
        .plus(slow.times(SLOW_DUTIES_PER_HOUR))
        .plus(sweep);
    let own_uploads = upload.times(UPLOADS_PER_MEMBER_HOUR);
    let busy_hour = idle_hour.plus(own_uploads).plus(hinted.times(others_uploads));
    let busy_hour_without_gossip = idle
        .times(CHECKS_PER_BUSY_HOUR_WITHOUT_GOSSIP)
        .plus(slow.times(SLOW_DUTIES_PER_HOUR))
        .plus(sweep)
        .plus(own_uploads)
        .plus(unhinted.times(others_uploads));
    let month = |busy: Cost| busy.times(BUSY_HOURS).plus(idle_hour.times(IDLE_HOURS)).dollars();

    println!("\n{member_count} members, per person:");
    println!("  idle check: {idle}; 10-minute duties add {slow}; sweep: {sweep}");
    println!("  own upload: {upload}; someone's upload, hinted: {hinted}, gossip lost: {unhinted}");
    println!("  idle hour: {idle_hour}");
    println!("  busy hour, gossip healthy: {busy_hour}");
    println!("  busy hour, no gossip: {busy_hour_without_gossip}");
    println!(
        "  S3 a month: ${:.2} with gossip healthy, ${:.2} with no gossip",
        month(busy_hour),
        month(busy_hour_without_gossip)
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "slow: builds workspaces of 10 and 50 members"]
async fn sync_budget_at_ten_and_fifty_members() {
    for member_count in [scaled(10), scaled(50)] {
        budget_at(member_count.max(3)).await;
    }
}
