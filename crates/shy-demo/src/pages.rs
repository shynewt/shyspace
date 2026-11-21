//! The demo's pages: plans, guides and notes that show what pages can hold
//! (headings, checklists, tables, callouts and code).

use anyhow::{Context, Result};
use shy_core::{Edit, Workspace, ops::EntityId};

use crate::cast::{ADMIN, Cast};

pub struct Pages {
    pub launch_plan: EntityId,
    pub brand: EntityId,
    pub handbook: EntityId,
    pub playbook: EntityId,
}

fn write_page(member: &Workspace, title: &str, markdown: &str) -> Result<EntityId> {
    let page = member.create_page(title, None).context("creating a page")?;
    member
        .edit_page(&page, Edit::Append(markdown))
        .context("writing a page")?;
    Ok(page)
}

pub fn write_pages(cast: &Cast) -> Result<Pages> {
    let launch_plan = write_page(
        cast.get(ADMIN),
        "Launch plan",
        "## Goal\n\nShip **Lumen 1.0** to the first 500 customers.\n\n\
         > [!IMPORTANT]\n> The launch date stays private until the website is ready.\n\n\
         ## Timeline\n\n- [x] Feature freeze\n- [x] Beta with design partners\n- [ ] Release candidate\n\
         - [ ] Launch day\n\n## Owners\n\n| Area | Who | Status |\n| --- | --- | :---: |\n\
         | Website | Maya | 🟡 |\n| Release | Theo | 🟢 |\n| Support | Priya | 🟢 |\n| Press kit | Maya | 🔴 |\n",
    )?;
    let brand = write_page(
        cast.get("Maya"),
        "Brand guidelines",
        "## Colours\n\nOur accent is a calm violet. Use it for one thing per screen: the next step.\n\n\
         > [!TIP]\n> If two things on a screen are violet, one of them shouldn't be.\n\n\
         ## Icons\n\n- 1.5 px strokes, rounded ends\n- 20 px grid, 2 px padding\n- No fills except for state\n\n\
         ## Voice\n\n| Say | Don't say |\n| --- | --- |\n| Saved | Your changes have been successfully saved |\n\
         | Can't connect | Error 503 |\n",
    )?;
    let handbook = write_page(
        cast.get("Theo"),
        "Engineering handbook",
        "## Getting started\n\n```sh\ngit clone git@example.com:northwind/lumen.git\ncd lumen && make dev\n```\n\n\
         > [!WARNING]\n> Never commit the `.env` file. It holds the staging keys.\n\n\
         ## Reviews\n\n1. Keep pull requests small.\n2. Every change comes with a test.\n\
         3. Two approvals for anything touching payments.\n\n\
         ## Errors\n\nAdd context to every error, so the log says what was being attempted:\n\n\
         ```rust\nlet page = store\n    .load(&id)\n    .with_context(|| format!(\"loading page {id}\"))?;\n```\n",
    )?;
    let playbook = write_page(
        cast.get("Priya"),
        "Support playbook",
        "## Response times\n\n| Plan | First reply | Fix or workaround |\n| --- | ---: | ---: |\n\
         | Free | 2 days | best effort |\n| Team | 8 hours | 3 days |\n| Business | 1 hour | 1 day |\n\n\
         > [!NOTE]\n> Weekends count for Business only.\n\n\
         ## Before you reply\n\n- [ ] Read the whole thread\n- [ ] Check the known issues page\n\
         - [ ] Reproduce it if you can\n",
    )?;
    write_page(
        cast.get("Theo"),
        "Meeting notes: release sync",
        "**Who:** Ada, Theo, Sam, Priya\n\n## Decisions\n\n- Release candidate on the 2nd\n\
         - Upload retries ship behind a setting\n\n## Action items\n\n- [x] Theo: freeze the release branch\n\
         - [ ] Sam: review the upload retry change\n- [ ] Priya: update the help center\n- [ ] Ada: sign off the launch plan\n",
    )?;
    Ok(Pages {
        launch_plan,
        brand,
        handbook,
        playbook,
    })
}
