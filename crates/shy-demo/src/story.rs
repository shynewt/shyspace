//! What happens in the demo workspace: a small studio getting a launch out of
//! the door. It leaves something to see on every screen: unread messages and
//! mentions, threads, reactions, a private channel, DMs, notes, pages,
//! statuses, sidebar sections, and reminders both due and coming up.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use shy_core::{
    Edit, Workspace,
    ops::EntityId,
    views::{NotifyLevel, StatusUpdate},
};

use crate::cast::{ADMIN, Cast};

const MINUTE_MS: i64 = 60 * 1000;
const HOUR_MS: i64 = 60 * MINUTE_MS;
const SETTLE_ROUNDS: u32 = 20;

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

fn say(member: &Workspace, channel: &str, text: &str) -> Result<EntityId> {
    member
        .send_message(channel, text, None)
        .with_context(|| format!("sending “{}”", text.chars().take(40).collect::<String>()))
}

fn reply(member: &Workspace, channel: &str, root: &str, text: &str) -> Result<EntityId> {
    member
        .send_message(channel, text, Some(root.to_owned()))
        .context("sending a reply")
}

fn react(member: &Workspace, message: &str, emoji: &str) -> Result<()> {
    member.react(message, emoji, true).context("reacting")
}

fn write_page(member: &Workspace, title: &str, markdown: &str) -> Result<EntityId> {
    let page = member.create_page(title, None).context("creating a page")?;
    member
        .edit_page(&page, Edit::Append(markdown))
        .context("writing a page")?;
    Ok(page)
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
    talk_directly(cast).await?;
    set_statuses(cast)?;
    cast.sync_everyone().await?;
    say(
        cast.turn("Theo").await?,
        &channel_id(cast.get(ADMIN), "general")?,
        &format!("For those on the launch: the timeline is settled, see shy://message/{launch}"),
    )?;
    cast.sync_everyone().await?;
    arrange_for_ada(cast, &chat)
}

struct Pages {
    launch_plan: EntityId,
    brand: EntityId,
    handbook: EntityId,
}

fn write_pages(cast: &Cast) -> Result<Pages> {
    let launch_plan = write_page(
        cast.get(ADMIN),
        "Launch plan",
        "## Goal\n\nShip **Lumen 1.0** to the first 500 customers.\n\n\
         ## Timeline\n\n- [x] Feature freeze\n- [x] Beta with design partners\n- [ ] Release candidate\n\
         - [ ] Launch day\n\n## Owners\n\n| Area | Who |\n| --- | --- |\n| Website | Maya |\n\
         | Release | Theo |\n| Support | Priya |\n",
    )?;
    let brand = write_page(
        cast.get("Maya"),
        "Brand guidelines",
        "## Colours\n\nOur accent is a calm violet. Use it for one thing per screen: the next step.\n\n\
         ## Icons\n\n- 1.5 px strokes, rounded ends\n- 20 px grid, 2 px padding\n- No fills except for state\n\n\
         > Simple beats clever. If an icon needs a label to make sense, keep the label and drop the icon.\n",
    )?;
    let handbook = write_page(
        cast.get("Theo"),
        "Engineering handbook",
        "## Getting started\n\n```sh\ngit clone git@example.com:northwind/lumen.git\ncd lumen && make dev\n```\n\n\
         ## Reviews\n\n1. Keep pull requests small.\n2. Every change comes with a test.\n\
         3. Two approvals for anything touching payments.\n",
    )?;
    Ok(Pages {
        launch_plan,
        brand,
        handbook,
    })
}

/// Messages later used to set Ada's read position and reminders.
struct Chat {
    general_first: EntityId,
    ask_for_ada: EntityId,
    beta_question: EntityId,
    random_last: EntityId,
}

async fn chat_in_public(cast: &Cast, pages: &Pages) -> Result<Chat> {
    let ada = cast.get(ADMIN);
    let general = channel_id(ada, "general")?;
    let general_first = say(
        cast.turn("Maya").await?,
        &general,
        "Morning all ☀️ The design review moves to **Thursday at 2pm**.",
    )?;
    react(cast.turn("Theo").await?, &general_first, "👍")?;
    react(cast.turn("Sam").await?, &general_first, "👍")?;
    say(
        cast.turn("Theo").await?,
        &general,
        "Thanks! I'll bring the new onboarding flow.",
    )?;
    let ask_for_ada = say(
        cast.turn("Priya").await?,
        &general,
        &format!(
            "{} could you look over the {} before Friday?",
            mention(cast, ADMIN),
            page_link("Launch plan", &pages.launch_plan)
        ),
    )?;
    let coffee = say(
        cast.turn("Sam").await?,
        &general,
        "The coffee machine on the third floor is fixed, finally ☕",
    )?;
    react(cast.turn("Priya").await?, &coffee, "🎉")?;
    react(cast.turn("Maya").await?, &coffee, "☕")?;

    let design = channel_id(ada, "design")?;
    let icons = say(
        cast.turn("Maya").await?,
        &design,
        &format!(
            "First pass at the new icons is in {}. Feedback welcome!",
            page_link("Brand guidelines", &pages.brand)
        ),
    )?;
    reply(
        cast.turn("Theo").await?,
        &design,
        &icons,
        "Love the rounded ends. Much friendlier.",
    )?;
    reply(
        cast.turn("Maya").await?,
        &design,
        &icons,
        &format!(
            "Thanks! {} can you check them in dark mode?",
            mention(cast, ADMIN)
        ),
    )?;
    react(cast.turn("Priya").await?, &icons, "😍")?;

    let engineering = channel_id(ada, "engineering")?;
    let faster = say(
        cast.turn("Theo").await?,
        &engineering,
        "Sync is about 30% faster after yesterday's change 🚀",
    )?;
    react(cast.turn("Sam").await?, &faster, "🚀")?;
    let beta_question = say(
        cast.turn("Sam").await?,
        &engineering,
        "Nice. Are we still on track for the release candidate?",
    )?;
    say(
        cast.turn("Scout").await?,
        &engineering,
        &format!(
            "Open issues this week: **3 bugs**, 2 in review. The oldest is the export crash \
             on large pages. Setup steps are in the {}.",
            page_link("Engineering handbook", &pages.handbook)
        ),
    )?;
    say(
        cast.turn("Priya").await?,
        &engineering,
        "To reproduce the export crash:\n```sh\nlumen export --all big-page.md\n```",
    )?;

    let random = channel_id(ada, "random")?;
    let lunch = say(
        cast.turn("Sam").await?,
        &random,
        "Who's up for lunch at the new ramen place? 🍜",
    )?;
    react(cast.turn("Maya").await?, &lunch, "🙋")?;
    react(cast.turn("Theo").await?, &lunch, "🙋")?;
    let random_last = say(cast.turn("Maya").await?, &random, "Count me in, 12:30?")?;

    Ok(Chat {
        general_first,
        ask_for_ada,
        beta_question,
        random_last,
    })
}

/// A private channel for Ada, Maya and Theo; the others can't read it.
async fn plan_in_private(cast: &Cast, pages: &Pages) -> Result<EntityId> {
    let ada = cast.get(ADMIN);
    let channel = ada
        .create_private_channel("launch-plan", &[cast.id("Maya"), cast.id("Theo")])
        .context("creating the private channel")?;
    wait_until_in(cast, &channel, &["Maya", "Theo"]).await?;
    say(
        cast.turn(ADMIN).await?,
        &channel,
        &format!(
            "The draft timeline is in the {}.",
            page_link("Launch plan", &pages.launch_plan)
        ),
    )?;
    let settled = say(
        cast.turn("Maya").await?,
        &channel,
        "Looks good. Can we keep the date quiet until the website is ready?",
    )?;
    say(
        cast.turn("Theo").await?,
        &channel,
        "Agreed. Release candidate on the 2nd, launch on the 9th.",
    )?;
    Ok(settled)
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

async fn talk_directly(cast: &Cast) -> Result<()> {
    let ada = cast.id(ADMIN);
    let with_priya = cast
        .turn("Priya")
        .await?
        .open_dm(std::slice::from_ref(&ada))
        .context("opening a DM")?;
    say(
        cast.turn("Priya").await?,
        &with_priya,
        "Hey! Have you got a minute later to go through the support budget?",
    )?;

    let group = cast
        .turn("Theo")
        .await?
        .open_dm(&[ada, cast.id("Sam")])
        .context("opening a group DM")?;
    say(
        cast.turn("Theo").await?,
        &group,
        "Pairing on the release script tomorrow morning?",
    )?;
    wait_until_in(cast, &group, &["Sam"]).await?;
    say(cast.turn("Sam").await?, &group, "I'm in. 10am?")?;

    let notes = cast.get(ADMIN).open_notes().context("opening notes")?;
    say(
        cast.turn(ADMIN).await?,
        &notes,
        "Ideas for after launch:\n- dark mode for pages\n- a weekly digest\n- keyboard shortcuts cheat sheet",
    )?;
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
