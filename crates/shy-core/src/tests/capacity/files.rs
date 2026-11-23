//! Many files, and one at the size limit: sharing, listing and opening.

use std::{io::Write, time::Instant};

use super::{
    counting_store::counting_bucket, database_bytes, median_time, megabytes, ms, peak_memory, per_second,
    scaled,
};
use crate::{
    files::MAX_FILE_BYTES,
    tests::support::*,
    transport::bucket::Bucket,
    workspace::{FileSource, NewFile},
};

const PAGE: u32 = 50;
const SMALL_FILE_BYTES: usize = 2_000;

fn small_file(index: usize) -> NewFile {
    NewFile {
        name: format!("notes-{index}.txt"),
        media_type: "text/plain".into(),
        source: FileSource::Bytes(vec![b'a' + (index % 26) as u8; SMALL_FILE_BYTES]),
    }
}

async fn snapshot_bytes(bucket: &Bucket) -> u64 {
    bucket
        .recovery_snapshot()
        .await
        .unwrap()
        .map_or(0, |bytes| bytes.len() as u64)
}

#[tokio::test]
#[ignore = "capacity probe, see tests/capacity/mod.rs"]
async fn many_files_and_one_large_file() {
    let steps = [scaled(1_000), scaled(10_000)];
    let root = tempfile::tempdir().unwrap();
    let (store, bucket) = counting_bucket();
    let ada = create_admin(&root.path().join("ada"), &bucket).await;
    let bob = join(&ada, &root.path().join("bob"), &bucket, "Bob", false).await;
    let channel = bob.create_channel("shared").unwrap();
    ada.recovery_kit().await.unwrap();
    let snapshot_before = snapshot_bytes(&bucket).await;
    println!("\n== Files: Bob shares small files, Ada lists them ==");
    let mut shared = 0;
    for step in steps {
        let start = Instant::now();
        for index in shared..step {
            bob.send_files(&channel, "", None, vec![small_file(index)])
                .await
                .unwrap();
        }
        let share_time = start.elapsed();
        let new_files = step - shared;
        shared = step;
        bob.sync().await;
        let start = Instant::now();
        ada.sync().await;
        let ingest_time = start.elapsed();
        assert_eq!(ada.files().unwrap().len(), step);
        println!("\n-- {step} files --");
        println!("Bob shares:                {}", per_second(new_files, share_time));
        println!(
            "Ada applies:               {}",
            per_second(new_files, ingest_time)
        );
        println!(
            "Ada's database:            {}",
            megabytes(database_bytes(&root.path().join("ada")))
        );
        println!(
            "Files view (all files):    {}",
            ms(median_time(|| ada.files().unwrap()))
        );
        println!(
            "Open the channel ({PAGE}):    {}",
            ms(median_time(|| ada.messages(&channel, None, PAGE).unwrap()))
        );
    }

    // Joining is a membership change, so the admin refreshes its recovery snapshot.
    join(&ada, &root.path().join("carol"), &bucket, "Carol", false).await;
    ada.sync().await;
    println!(
        "\nAdmin's recovery snapshot:  {} before the files, {} after",
        megabytes(snapshot_before),
        megabytes(snapshot_bytes(&bucket).await)
    );

    let large_path = root.path().join("large.bin");
    let mut large = std::fs::File::create(&large_path).unwrap();
    let block: Vec<u8> = (0..1024 * 1024).map(|index| (index % 251) as u8).collect();
    for _ in 0..MAX_FILE_BYTES / 1024 / 1024 {
        large.write_all(&block).unwrap();
    }
    drop(large);
    let start = Instant::now();
    bob.send_files(
        &channel,
        "the large one",
        None,
        vec![NewFile {
            name: "large.bin".into(),
            media_type: "application/octet-stream".into(),
            source: FileSource::Path(large_path.clone()),
        }],
    )
    .await
    .unwrap();
    let share_time = start.elapsed();
    bob.sync().await;
    ada.sync().await;
    let file = ada
        .files()
        .unwrap()
        .into_iter()
        .find(|file| file.name == "large.bin")
        .expect("Ada sees the large file");
    let saved = root.path().join("saved.bin");
    let before = store.requests();
    let start = Instant::now();
    ada.save_file(&file.id, &saved).await.unwrap();
    let save_time = start.elapsed();
    let save_requests = store.requests().since(&before);
    assert_eq!(
        std::fs::read(&saved).unwrap(),
        std::fs::read(&large_path).unwrap()
    );
    println!("\n== One {} file ==", megabytes(MAX_FILE_BYTES));
    println!("Share (seal, cache, upload):  {}", ms(share_time));
    println!(
        "Save it on Ada's device:      {} ({save_requests})",
        ms(save_time)
    );
    println!("Peak memory of the test:      {}", peak_memory());
}
