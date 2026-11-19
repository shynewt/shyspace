//! Sidebar sections: personal groups of channels.

use super::support::*;
use crate::views::SectionSort;

fn layout(workspace: &crate::Workspace) -> Vec<(String, Vec<String>)> {
    let channels = workspace.channels().unwrap();
    let name_of = |id: &str| {
        channels
            .iter()
            .find(|channel| channel.id == id)
            .unwrap()
            .name
            .clone()
    };
    workspace
        .sidebar_sections()
        .unwrap()
        .into_iter()
        .map(|section| {
            (
                section.name,
                section.channels.iter().map(|id| name_of(id)).collect(),
            )
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn channels_can_be_grouped_into_personal_sections() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let design = admin.create_channel("design").unwrap();
    let alpha = admin.create_channel("alpha").unwrap();
    sync_all(&[&admin, &bob]).await;

    let projects = admin.create_section("Projects").unwrap();
    admin.move_channel(&design, Some(&projects)).unwrap();
    admin.move_channel(&alpha, Some(&projects)).unwrap();
    assert_eq!(
        layout(&admin),
        vec![
            ("Channels".to_string(), vec!["general".to_string()]),
            (
                "Projects".to_string(),
                vec!["alpha".to_string(), "design".to_string()]
            ),
        ],
        "the built-in section first; names sort A to Z by default"
    );
    assert_eq!(layout(&bob)[0].1.len(), 3, "sections are personal");

    admin.send_message(&design, "fresh news", None).unwrap();
    admin.set_section_sort(&projects, SectionSort::Recent).unwrap();
    assert_eq!(
        layout(&admin)[1].1,
        vec!["design".to_string(), "alpha".to_string()]
    );

    admin.set_section_collapsed(&projects, true).unwrap();
    assert!(admin.sidebar_sections().unwrap()[1].is_collapsed);
    admin.rename_section(&projects, "Work").unwrap();
    admin.delete_section(&projects).unwrap();
    assert_eq!(
        layout(&admin),
        vec![(
            "Channels".to_string(),
            vec!["alpha".into(), "design".into(), "general".into()]
        )]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deleted_channel_leaves_its_section() {
    let (_root, _bucket, admin, bob) = admin_and_bob().await;
    let random = bob.create_channel("random").unwrap();
    sync_all(&[&admin, &bob]).await;
    let mine = admin.create_section("Mine").unwrap();
    admin.move_channel(&random, Some(&mine)).unwrap();

    bob.delete_channel(&random).unwrap();
    sync_all(&[&admin, &bob]).await;
    assert!(admin.sidebar_sections().unwrap()[1].channels.is_empty());
}
