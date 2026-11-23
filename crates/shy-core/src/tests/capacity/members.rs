//! Many members: how the join answer, the admin's recovery snapshot and
//! every sync pass grow as people join and leave.

use std::{path::Path, time::Duration};

use super::{
    counting_store::{Requests, counting_bucket},
    megabytes, ms, scaled,
};
use crate::{Workspace, tests::support::*, transport::bucket::Bucket};

/// Join answers larger than this are refused (`transport/presigned.rs`).
const MAX_ANSWER_BYTES: f64 = 32.0 * 1024.0 * 1024.0;
const TURNOVER_PERCENT: usize = 20;

struct Joined {
    member: Workspace,
    answer_bytes: usize,
    approve_time: Duration,
}

/// Like `support::join`, but keeps the admin's answer size and approval time.
async fn join_measured(admin: &Workspace, dir: &Path, bucket: &Bucket, name: &str) -> Joined {
    let invite = admin.create_test_invite(name, Duration::from_secs(600)).unwrap();
    let token = invite.token.clone();
    let joining = start_join(invite, dir, bucket, name, false);
    wait_for_request(admin, &token).await;
    let start = std::time::Instant::now();
    admin.approve_invite(&token).await.unwrap();
    let approve_time = start.elapsed();
    let answer_bytes = bucket
        .invite_answer(&token)
        .await
        .unwrap()
        .expect("an answer")
        .len();
    let member = joining.await.unwrap().unwrap();
    // Their first ops tell the admin the answer arrived, so it stops re-sending it.
    member.sync().await;
    assert_eq!(member.status().error, None);
    Joined {
        member,
        answer_bytes,
        approve_time,
    }
}

async fn snapshot_bytes(admin: &Workspace, bucket: &Bucket) -> usize {
    admin.sync().await;
    bucket
        .recovery_snapshot()
        .await
        .unwrap()
        .map_or(0, |bytes| bytes.len())
}

fn active_members(admin: &Workspace) -> usize {
    admin
        .members()
        .unwrap()
        .iter()
        .filter(|member| member.active)
        .count()
}

async fn idle_pass(member: &Workspace, requests: impl Fn() -> Requests) -> Requests {
    let before = requests();
    member.sync().await;
    requests().since(&before)
}

/// The member count where a fitted `a + b·n + c·n²` reaches `limit`.
fn where_quadratic_reaches(points: [(f64, f64); 3], limit: f64) -> f64 {
    let [(x1, y1), (x2, y2), (x3, y3)] = points;
    let c = ((y3 - y1) / (x3 - x1) - (y2 - y1) / (x2 - x1)) / (x3 - x2);
    let b = (y2 - y1) / (x2 - x1) - c * (x1 + x2);
    let a = y1 - b * x1 - c * x1 * x1;
    (-b + (b * b - 4.0 * c * (a - limit)).sqrt()) / (2.0 * c)
}

#[tokio::test]
#[ignore = "capacity probe, see tests/capacity/mod.rs"]
async fn membership_as_people_join_and_leave() {
    let checkpoints = [scaled(10), scaled(25), scaled(50), scaled(100), scaled(150)];
    let root = tempfile::tempdir().unwrap();
    let (store, bucket) = counting_bucket();
    let requests = || store.requests();
    let ada = create_admin(&root.path().join("ada"), &bucket).await;
    ada.recovery_kit().await.unwrap();
    let mut member_ids = Vec::new();
    let mut fitted = Vec::new();
    println!("\n== Members join one by one ==");
    println!("members | join answer | approve | recovery snapshot | idle pass (member) | idle pass (admin)");
    let mut count = 1;
    for checkpoint in checkpoints {
        let mut last = None;
        while count < checkpoint {
            let name = format!("Member {count}");
            let joined = join_measured(&ada, &root.path().join(format!("m{count}")), &bucket, &name).await;
            member_ids.push(joined.member.my_id());
            last = Some(joined);
            count += 1;
        }
        let Some(joined) = last else { continue };
        let member_pass = idle_pass(&joined.member, requests).await;
        let admin_pass = idle_pass(&ada, requests).await;
        println!(
            "{count:>7} | {:>11} | {:>7} | {:>17} | {member_pass} | {admin_pass}",
            megabytes(joined.answer_bytes as u64),
            ms(joined.approve_time),
            megabytes(snapshot_bytes(&ada, &bucket).await as u64),
        );
        assert_eq!(active_members(&ada), count);
        fitted.push((count as f64, joined.answer_bytes as f64));
    }
    assert!(
        fitted.len() >= 3,
        "too few members to fit a curve; raise SHY_CAPACITY_SCALE"
    );
    let points = [fitted[0], fitted[fitted.len() / 2], fitted[fitted.len() - 1]];
    println!(
        "Fitted join answer reaches the 32 MiB limit at about {:.0} members, if nobody leaves",
        where_quadratic_reaches(points, MAX_ANSWER_BYTES)
    );

    let leaving = (member_ids.len() * TURNOVER_PERCENT / 100).max(1);
    let start = std::time::Instant::now();
    for id in member_ids.drain(..leaving) {
        ada.remove_member(&id).await.unwrap();
    }
    let remove_time = start.elapsed();
    let replacement = join_measured(&ada, &root.path().join("replacement"), &bucket, "Replacement").await;
    let changes = leaving + 1;
    let (_, before_bytes) = fitted[fitted.len() - 1];
    let growth = (replacement.answer_bytes as f64 - before_bytes) / changes as f64;
    println!("\n== {leaving} members leave, one joins ==");
    println!(
        "Removing one member:            {}",
        ms(remove_time / u32::try_from(leaving).unwrap())
    );
    println!(
        "Join answer now:                {}",
        megabytes(replacement.answer_bytes as u64)
    );
    println!(
        "Each membership change adds:    {growth:.0} bytes ({:.1} per current member)",
        growth / count as f64
    );
    println!(
        "Recovery snapshot now:          {}",
        megabytes(snapshot_bytes(&ada, &bucket).await as u64)
    );
    assert_eq!(active_members(&ada), count - leaving + 1);
}
