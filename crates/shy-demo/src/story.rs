//! What happens in the demo workspace: a small studio getting a launch out of
//! the door. It leaves something to see on every screen: unread messages and
//! mentions, threads, reactions, a private channel, DMs, notes, pages,
//! statuses, profiles, sidebar sections, and reminders both due and coming up.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use shy_core::{
    Workspace,
    ops::EntityId,
    profile::{ProfileDetails, WorkingHours},
    views::{NotifyLevel, StatusUpdate},
};

use crate::{
    cast::{ADMIN, Cast},
    pages::{Pages, write_pages},
};

const MINUTE_MS: i64 = 60 * 1000;
const HOUR_MS: i64 = 60 * MINUTE_MS;
const SETTLE_ROUNDS: u32 = 20;

/// Who says what, in order.
type Lines<'a> = &'a [(&'static str, &'a str)];

fn now_ms() -> Result<i64> {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("reading the clock")?;
    i64::try_from(since_epoch.as_millis()).context("the clock is out of range")
}

fn mention(cast: &Cast, name: &str) -> String {
    format!("[@{name}](shy://member/{})", cast.id(name))
}

fn page_link(title: &str, page: &str) -> String {
    format!("[{title}](shy://page/{page})")
}

fn channel_id(member: &Workspace, name: &str) -> Result<EntityId> {
    member
        .channels()?
        .into_iter()
        .find(|channel| channel.name == name)
        .map(|channel| channel.id)
        .with_context(|| format!("no #{name} channel"))
}

fn send(member: &Workspace, channel: &str, text: &str, reply_to: Option<&str>) -> Result<EntityId> {
    member
        .send_message(channel, text, reply_to.map(str::to_owned))
        .with_context(|| format!("sending “{}”", text.chars().take(40).collect::<String>()))
}

/// Each line as a message in `channel`, or in the thread under `reply_to`.
async fn converse(
    cast: &Cast,
    channel: &str,
    reply_to: Option<&str>,
    lines: Lines<'_>,
) -> Result<Vec<EntityId>> {
    let mut sent = Vec::new();
    for (speaker, text) in lines {
        sent.push(send(cast.turn(speaker).await?, channel, text, reply_to)?);
    }
    Ok(sent)
}

async fn react(cast: &Cast, message: &str, reactions: &[(&'static str, &str)]) -> Result<()> {
    for (who, emoji) in reactions {
        cast.turn(who)
            .await?
            .react(message, emoji, true)
            .context("reacting")?;
    }
    Ok(())
}

/// Unread messages, mentions and reminders are all from Ada's point of view.
pub async fn tell(cast: &Cast) -> Result<()> {
    let ada = cast.get(ADMIN);
    for name in ["design", "engineering", "random"] {
        ada.create_channel(name).context("creating a channel")?;
    }
    cast.sync_everyone().await?;

    let pages = write_pages(cast)?;
    cast.sync_everyone().await?;
    let chat = chat_in_public(cast, &pages).await?;
    let launch = plan_in_private(cast, &pages).await?;
    talk_directly(cast, &pages).await?;
    set_statuses(cast)?;
    share_profiles(cast)?;
    cast.sync_everyone().await?;
    send(
        cast.turn("Theo").await?,
        &channel_id(cast.get(ADMIN), "general")?,
        &format!("For those on the launch: the timeline is settled, see shy://message/{launch}"),
        None,
    )?;
    cast.sync_everyone().await?;
    arrange_for_ada(cast, &chat)
}

/// Messages later used to set Ada's read position and reminders.
struct Chat {
    general_first: EntityId,
    ask_for_ada: EntityId,
    beta_question: EntityId,
    random_last: EntityId,
}

async fn chat_in_public(cast: &Cast, pages: &Pages) -> Result<Chat> {
    let (general_first, ask_for_ada) = chat_in_general(cast, pages).await?;
    chat_in_design(cast, pages).await?;
    let beta_question = chat_in_engineering(cast, pages).await?;
    let random_last = chat_in_random(cast).await?;
    Ok(Chat {
        general_first,
        ask_for_ada,
        beta_question,
        random_last,
    })
}

async fn chat_in_general(cast: &Cast, pages: &Pages) -> Result<(EntityId, EntityId)> {
    let general = channel_id(cast.get(ADMIN), "general")?;
    let ask = format!(
        "{} could you look over the {} before Friday?",
        mention(cast, ADMIN),
        page_link("Launch plan", &pages.launch_plan)
    );
    let sent = converse(
        cast,
        &general,
        None,
        &[
            (
                "Maya",
                "Morning all ☀️ The design review moves to **Thursday at 2pm**.",
            ),
            ("Theo", "Thanks! I'll bring the new onboarding flow."),
            ("Priya", &ask),
            (
                "Sam",
                "The coffee machine on the third floor is fixed, finally ☕",
            ),
            (
                "Priya",
                "Please welcome our newest design partner, **Fernhill Labs** 🎉",
            ),
        ],
    )
    .await?;
    react(cast, &sent[0], &[("Theo", "👍"), ("Sam", "👍")]).await?;
    react(cast, &sent[3], &[("Priya", "🎉"), ("Maya", "☕")]).await?;
    react(cast, &sent[4], &[("Maya", "🎉"), ("Theo", "🎉"), ("Sam", "🙌")]).await?;
    converse(
        cast,
        &general,
        Some(&sent[4]),
        &[
            ("Maya", "Their team was lovely to work with in the beta."),
            ("Sam", "They found the export bug too 😅"),
            ("Priya", "They did! Their first feedback call is on Monday."),
        ],
    )
    .await?;
    converse(
        cast,
        &general,
        None,
        &[
            (
                "Theo",
                "Reminder: the release freeze starts **Wednesday**. Please merge what you can by Tuesday evening.",
            ),
            (
                "Scout",
                "Weekly summary: 14 pull requests merged, 3 bugs fixed.\n\n\
                 | Area | Merged | Still open |\n| --- | ---: | ---: |\n| Sync | 6 | 1 |\n\
                 | Pages | 5 | 0 |\n| Desktop | 3 | 1 |",
            ),
        ],
    )
    .await?;
    Ok((sent[0].clone(), sent[2].clone()))
}

async fn chat_in_design(cast: &Cast, pages: &Pages) -> Result<()> {
    let design = channel_id(cast.get(ADMIN), "design")?;
    let icons_text = format!(
        "First pass at the new icons is in {}. Feedback welcome!",
        page_link("Brand guidelines", &pages.brand)
    );
    let icons = converse(cast, &design, None, &[("Maya", &icons_text)]).await?;
    let dark_mode = format!(
        "Thanks! {} can you check them in dark mode?",
        mention(cast, ADMIN)
    );
    converse(
        cast,
        &design,
        Some(&icons[0]),
        &[
            ("Theo", "Love the rounded ends. Much friendlier."),
            ("Maya", &dark_mode),
        ],
    )
    .await?;
    react(cast, &icons[0], &[("Priya", "😍"), ("Sam", "👏")]).await?;
    let rest = converse(
        cast,
        &design,
        None,
        &[
            (
                "Maya",
                "One rule I'd like us all to keep for the marketing site:\n\n\
                 > [!TIP]\n> Use the violet only for the main call to action.",
            ),
            ("Theo", "Could the empty states use the same illustration style?"),
            ("Maya", "Yes! Drafts by Friday."),
            ("Sam", "The onboarding screens feel much calmer now 👏"),
        ],
    )
    .await?;
    react(cast, &rest[0], &[("Theo", "💯")]).await
}

/// Returns Sam's question about the release candidate, which Ada saves.
async fn chat_in_engineering(cast: &Cast, pages: &Pages) -> Result<EntityId> {
    let engineering = channel_id(cast.get(ADMIN), "engineering")?;
    let issues = format!(
        "Open issues this week: **3 bugs**, 2 in review. The oldest is the export crash \
         on large pages. Setup steps are in the {}.",
        page_link("Engineering handbook", &pages.handbook)
    );
    let sent = converse(
        cast,
        &engineering,
        None,
        &[
            ("Theo", "Sync is about 30% faster after yesterday's change 🚀"),
            ("Sam", "Nice. Are we still on track for the release candidate?"),
            ("Scout", &issues),
            (
                "Priya",
                "To reproduce the export crash:\n```sh\nlumen export --all big-page.md\n```",
            ),
        ],
    )
    .await?;
    react(cast, &sent[0], &[("Sam", "🚀"), ("Maya", "🚀")]).await?;
    converse(
        cast,
        &engineering,
        Some(&sent[3]),
        &[
            (
                "Theo",
                "Found it: we read the whole page into memory. Writing it in chunks fixes it:\n\
                 ```rust\nfor chunk in page.chunks(64 * 1024) {\n    writer.write_all(chunk)?;\n}\n```",
            ),
            ("Priya", "Confirmed, fixed on my machine ✅"),
        ],
    )
    .await?;
    let rest = converse(
        cast,
        &engineering,
        None,
        &[
            (
                "Sam",
                "Can someone review #412? It's the retry logic for uploads.",
            ),
            ("Theo", "On it after lunch."),
            (
                "Scout",
                "The flaky upload test was waiting on a real timer. A fake one fixes it:\n\
                 ```ts\nvi.useFakeTimers();\nawait vi.advanceTimersByTimeAsync(1_000);\n\
                 expect(retries).toBe(1);\n```",
            ),
        ],
    )
    .await?;
    react(cast, &rest[2], &[("Sam", "🙏")]).await?;
    Ok(sent[1].clone())
}

/// Returns the last message, up to which Ada has read.
async fn chat_in_random(cast: &Cast) -> Result<EntityId> {
    let random = channel_id(cast.get(ADMIN), "random")?;
    let sent = converse(
        cast,
        &random,
        None,
        &[
            ("Sam", "Who's up for lunch at the new ramen place? 🍜"),
            ("Maya", "Count me in, 12:30?"),
            (
                "Theo",
                "Anyone reading something good? I just finished *The Mythical Man-Month* again 📚",
            ),
            ("Priya", "Project Hail Mary! Couldn't put it down."),
            ("Maya", "Photo walk on Saturday morning if the weather holds 📷"),
            ("Sam", "I'm in!"),
        ],
    )
    .await?;
    react(cast, &sent[0], &[("Maya", "🙋"), ("Theo", "🙋")]).await?;
    react(cast, &sent[4], &[("Priya", "📷")]).await?;
    sent.last().cloned().context("random has messages")
}

/// A private channel for Ada, Maya and Theo; the others can't read it.
async fn plan_in_private(cast: &Cast, pages: &Pages) -> Result<EntityId> {
    let ada = cast.get(ADMIN);
    let channel = ada
        .create_private_channel("launch-plan", &[cast.id("Maya"), cast.id("Theo")])
        .context("creating the private channel")?;
    wait_until_in(cast, &channel, &["Maya", "Theo"]).await?;
    let timeline = format!(
        "The draft timeline is in the {}.",
        page_link("Launch plan", &pages.launch_plan)
    );
    let sent = converse(
        cast,
        &channel,
        None,
        &[
            (ADMIN, &timeline),
            (
                "Maya",
                "Looks good. Can we keep the date quiet until the website is ready?",
            ),
            ("Theo", "Agreed. Release candidate on the 2nd, launch on the 9th."),
            (ADMIN, "Who wants the press kit?"),
            ("Maya", "I'll take it."),
            ("Theo", "And I'll write the release notes."),
        ],
    )
    .await?;
    Ok(sent[1].clone())
}

/// Syncs until each of `names` can read `channel`, which takes a few rounds
/// while they announce their keys and receive the channel's.
async fn wait_until_in(cast: &Cast, channel: &str, names: &[&str]) -> Result<()> {
    for _ in 0..SETTLE_ROUNDS {
        cast.sync_everyone().await?;
        let mut can_all_read = true;
        for name in names {
            can_all_read &= cast
                .get(name)
                .channels()?
                .iter()
                .any(|listed| listed.id == channel);
        }
        if can_all_read {
            return Ok(());
        }
    }
    bail!("{} never got into the private channel", names.join(" and "))
}

async fn open_dm(cast: &Cast, opener: &'static str, with: &[&str]) -> Result<EntityId> {
    let members: Vec<String> = with.iter().map(|name| cast.id(name)).collect();
    let dm = cast
        .turn(opener)
        .await?
        .open_dm(&members)
        .context("opening a DM")?;
    wait_until_in(cast, &dm, with).await?;
    Ok(dm)
}

async fn talk_directly(cast: &Cast, pages: &Pages) -> Result<()> {
    let with_priya = open_dm(cast, "Priya", &[ADMIN]).await?;
    let playbook = format!(
        "Also, I wrote up the {}. Does it look right to you?",
        page_link("Support playbook", &pages.playbook)
    );
    converse(
        cast,
        &with_priya,
        None,
        &[
            (
                "Priya",
                "Hey! Have you got a minute later to go through the support budget?",
            ),
            ("Priya", &playbook),
        ],
    )
    .await?;

    let group = open_dm(cast, "Theo", &[ADMIN, "Sam"]).await?;
    converse(
        cast,
        &group,
        None,
        &[
            ("Theo", "Pairing on the release script tomorrow morning?"),
            ("Sam", "I'm in. 10am?"),
            ("Theo", "10 works. I'll book the small room."),
        ],
    )
    .await?;

    let with_maya = open_dm(cast, "Maya", &[ADMIN]).await?;
    converse(
        cast,
        &with_maya,
        None,
        &[("Maya", "Quick one: are you OK with violet for the launch banner?")],
    )
    .await?;

    let with_scout = open_dm(cast, "Scout", &[ADMIN]).await?;
    converse(
        cast,
        &with_scout,
        None,
        &[(
            "Scout",
            "Morning digest: 2 mentions and 1 review request are waiting for you.",
        )],
    )
    .await?;

    let notes = cast.get(ADMIN).open_notes().context("opening notes")?;
    converse(
        cast,
        &notes,
        None,
        &[(
            ADMIN,
            "Ideas for after launch:\n- dark mode for pages\n- a weekly digest\n- keyboard shortcuts cheat sheet",
        )],
    )
    .await?;
    Ok(())
}

fn set_statuses(cast: &Cast) -> Result<()> {
    let now = now_ms()?;
    cast.get("Maya")
        .set_status(StatusUpdate {
            emoji: "🎨",
            text: "Deep in design work",
            until_ms: Some(now + 3 * HOUR_MS),
            alerts: NotifyLevel::Mentions,
        })
        .context("setting a status")?;
    cast.get("Sam")
        .set_status(StatusUpdate {
            emoji: "🌴",
            text: "On holiday until Monday",
            until_ms: Some(now + 72 * HOUR_MS),
            alerts: NotifyLevel::Nothing,
        })
        .context("setting a status")
}

const WEEKDAYS: u8 = 0b001_1111;

fn hours(days: u8, start_hour: u16, end_hour: u16) -> Option<WorkingHours> {
    Some(WorkingHours {
        days,
        start_minute: start_hour * 60,
        end_minute: end_hour * 60,
    })
}

/// Profiles in several time zones, one with night shifts across midnight.
/// Ada fills in only her title, so her profile still suggests what to add.
fn share_profiles(cast: &Cast) -> Result<()> {
    let profiles = [
        (
            ADMIN,
            ProfileDetails {
                title: "Founder".into(),
                ..ProfileDetails::default()
            },
        ),
        (
            "Maya",
            ProfileDetails {
                title: "Product designer".into(),
                pronouns: "she/her".into(),
                bio: "Designs the calm parts of Northwind. Sketches on paper first, always.\nPortfolio: https://maya.example.com".into(),
                email: "maya@northwind.example".into(),
                website: "https://maya.example.com".into(),
                location: "Lisbon studio".into(),
                time_zone: "Europe/Lisbon".into(),
                working_hours: hours(WEEKDAYS, 9, 17),
                ..ProfileDetails::default()
            },
        ),
        (
            "Theo",
            ProfileDetails {
                title: "Engineer".into(),
                pronouns: "he/him".into(),
                email: "theo@northwind.example".into(),
                location: "Remote, Tokyo".into(),
                time_zone: "Asia/Tokyo".into(),
                working_hours: hours(WEEKDAYS, 10, 18),
                ..ProfileDetails::default()
            },
        ),
        (
            "Priya",
            ProfileDetails {
                title: "Marketing lead".into(),
                pronouns: "she/her".into(),
                phone: "+1 (212) 555-0142".into(),
                location: "Remote, Brooklyn".into(),
                time_zone: "America/New_York".into(),
                working_hours: hours(WEEKDAYS, 9, 17),
                ..ProfileDetails::default()
            },
        ),
        (
            "Sam",
            ProfileDetails {
                title: "Support".into(),
                pronouns: "they/them".into(),
                bio: "Night shifts, so the queue is empty by morning.".into(),
                time_zone: "Europe/Berlin".into(),
                working_hours: hours(0b100_1111, 22, 6),
                ..ProfileDetails::default()
            },
        ),
    ];
    for (name, details) in profiles {
        cast.get(name)
            .set_profile_details(&details)
            .with_context(|| format!("sharing {name}'s profile"))?;
    }
    Ok(())
}

/// Ada has read the start of #general and all of #random, sorted two channels
/// into a section, and saved two messages: one reminder due, one coming up.
fn arrange_for_ada(cast: &Cast, chat: &Chat) -> Result<()> {
    let ada = cast.get(ADMIN);
    ada.mark_read(&channel_id(ada, "general")?, &chat.general_first)?;
    ada.mark_read(&channel_id(ada, "random")?, &chat.random_last)?;

    let projects = ada.create_section("Projects")?;
    for name in ["design", "engineering"] {
        ada.move_channel(&channel_id(ada, name)?, Some(&projects))?;
    }

    let now = now_ms()?;
    ada.save_for_later(&chat.ask_for_ada, Some(now - MINUTE_MS))?;
    ada.save_for_later(&chat.beta_question, Some(now + 2 * HOUR_MS))?;
    Ok(())
}
