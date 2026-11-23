//! A whole workspace's life on a real bucket provider, timed and with its
//! requests counted: create, join, messages until packs are written, typing
//! into a page, a private channel, a newcomer catching up, a removal and a
//! restore. Ignored by default; run it against a bucket you may write to:
//!
//! ```sh
//! SHY_PROVIDER_RUN=/path/to/provider.env cargo test -p shy-core --lib \
//!     provider_run -- --ignored --nocapture --test-threads=1
//! ```
//!
//! The file is the bucket probe's (`transport/bucket/probe.rs`). Nothing
//! from it is printed. Everything is written under `provider-run/<random>/`
//! and deleted afterwards, also when a step fails; on a bucket that keeps
//! versions the old versions stay, so remove them with the provider's tools
//! (for example `rclone delete --s3-versions`).

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use object_store::ObjectStore;

use super::counting_store::{CountingStore, Requests};
use crate::{
    BucketConfig, Edit, Networking, Workspace,
    tests::support::*,
    transport::bucket::{Bucket, Connector, probe::read_settings},
};

const MESSAGES: usize = 40;
const KEYSTROKE_BATCHES: usize = 60;

struct Step {
    name: &'static str,
    took: Duration,
    requests: Requests,
}

struct Timer {
    tally: Arc<CountingStore>,
    steps: Vec<Step>,
}

impl Timer {
    async fn step<T>(&mut self, name: &'static str, work: impl Future<Output = T>) -> T {
        let before = self.tally.requests();
        let started = Instant::now();
        let output = work.await;
        self.steps.push(Step {
            name,
            took: started.elapsed(),
            requests: self.tally.requests().since(&before),
        });
        output
    }
}

/// Every member reaches the real bucket under `run_prefix`, whatever their
/// stored settings say, and every request is counted into `tally`.
fn real_connector(settings: &BucketConfig, run_prefix: &str, tally: &Arc<CountingStore>) -> Connector {
    let (settings, run_prefix, tally) = (settings.clone(), run_prefix.to_owned(), tally.clone());
    Arc::new(move |workspace_id: &str, _: &BucketConfig| {
        let bucket = settings.connect(&format!("{run_prefix}/{workspace_id}"))?;
        Ok(bucket.wrapped(|store| -> Arc<dyn ObjectStore> { Arc::new(tally.wrapping(store)) }))
    })
}

async fn life_of_a_workspace(connect: Connector, bucket: &Bucket, timer: &mut Timer) {
    let root = tempfile::tempdir().unwrap();
    let admin = timer
        .step(
            "create the workspace",
            create_admin_connected(&root.path().join("ada"), connect.clone()),
        )
        .await;
    let bob = timer
        .step(
            "invite and join",
            join_connected(&admin, &root.path().join("bob"), bucket, connect.clone(), "Bob"),
        )
        .await;
    let general = channel_id(&admin, "general");
    timer
        .step("40 messages, uploaded one by one (packs at 16 and 32)", async {
            for number in 0..MESSAGES {
                bob.send_message(&general, &format!("message {number}"), None)
                    .unwrap();
                bob.push().await.unwrap();
            }
        })
        .await;
    let page = admin.create_page("Notes", None).unwrap();
    timer
        .step("typing into a page (60 uploads)", async {
            for batch in 0..KEYSTROKE_BATCHES {
                admin
                    .edit_page(&page, Edit::Append(&format!("word{batch} ")))
                    .unwrap();
                admin.push().await.unwrap();
            }
        })
        .await;
    timer
        .step("a private channel with 5 messages", async {
            // Bob's sealing key comes with his ops.
            admin.sync().await;
            let private = admin.create_private_channel("plans", &[bob.my_id()]).unwrap();
            for number in 0..5 {
                admin
                    .send_message(&private, &format!("secret {number}"), None)
                    .unwrap();
            }
            admin.sync().await;
            bob.sync().await;
        })
        .await;
    let carol = timer
        .step("a newcomer joins and catches up", async {
            let carol = join_connected(
                &admin,
                &root.path().join("carol"),
                bucket,
                connect.clone(),
                "Carol",
            )
            .await;
            carol.sync().await;
            carol
        })
        .await;
    assert_eq!(
        carol.state().store.last_seq(&bob.me()).unwrap(),
        bob.state().store.last_seq(&bob.me()).unwrap(),
        "the newcomer has all of Bob's log"
    );
    assert_eq!(carol.page_text(&page).unwrap(), admin.page_text(&page).unwrap());
    timer
        .step("remove a member", admin.remove_member(&carol.my_id()))
        .await
        .unwrap();
    let kit = admin.recovery_kit().await.unwrap();
    admin.sync().await;
    let admin_id = admin.me();
    drop(admin);
    let restored = timer
        .step(
            "restore the admin on a new device",
            Workspace::restore_with(
                &data_dir(&root.path().join("ada-new")),
                &kit,
                connect.clone(),
                Networking::Offline,
            ),
        )
        .await
        .unwrap();
    assert_eq!(restored.me(), admin_id);
    assert!(texts_in(&restored, &general).contains(&format!("message {}", MESSAGES - 1)));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a real bucket: set SHY_PROVIDER_RUN to a settings file"]
async fn provider_run() {
    let file = std::env::var("SHY_PROVIDER_RUN").expect("SHY_PROVIDER_RUN names a settings file");
    let settings = read_settings(&file).unwrap();
    let run_prefix = format!("provider-run/{}", hex::encode(rand::random::<[u8; 8]>()));
    let tally = Arc::new(CountingStore::default());
    let connect = real_connector(&settings, &run_prefix, &tally);
    let bucket = connect("workspace-id", &settings).unwrap();

    let outcome = tokio::spawn(async move {
        let mut timer = Timer {
            tally,
            steps: Vec::new(),
        };
        life_of_a_workspace(connect, &bucket, &mut timer).await;
        timer.steps
    })
    .await;

    let deleted = settings.connect(&run_prefix).unwrap().delete_everything().await;
    match deleted {
        Ok(count) => println!("cleaned up {count} objects"),
        Err(error) => println!("cleaning up failed: {error:#}"),
    }
    match outcome {
        Ok(steps) => {
            for step in steps {
                println!(
                    "{:>6.1}s  {}  ({})",
                    step.took.as_secs_f64(),
                    step.name,
                    step.requests
                );
            }
        }
        Err(panic) => std::panic::resume_unwind(panic.into_panic()),
    }
}
