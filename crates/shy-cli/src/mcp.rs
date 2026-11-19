//! Exposes a workspace to an AI agent as MCP tools. The agent acts as this
//! member: everything it writes is signed with this member's key.

use std::time::Duration;

use anyhow::{Context, Result};
use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::wrapper::Parameters,
    model::{ServerCapabilities, ServerConfig},
    schemars::{self, JsonSchema},
    tool, tool_handler, tool_router,
    transport::stdio,
};
use serde::Deserialize;
use shy_core::{
    Edit, Workspace,
    views::{Change, Message},
};

type ToolResult = Result<String, String>;

const DEFAULT_MESSAGE_LIMIT: u32 = 30;
const MAX_WAIT_SECONDS: u64 = 300;

#[derive(Clone)]
pub struct ShyTools {
    workspace: Workspace,
}

#[derive(Deserialize, JsonSchema)]
pub struct ReadMessages {
    /// Channel name (like "general") or id.
    channel: String,
    /// How many recent messages to return. Defaults to 30.
    limit: Option<u32>,
    /// Return messages older than this message id, to read further back.
    before: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SendMessage {
    /// Channel name or id.
    channel: String,
    /// Markdown text. Link a page with shy://page/<page id>.
    text: String,
    /// Id of the message this replies to.
    reply_to: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct React {
    message_id: String,
    emoji: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct PageRef {
    /// Page id or exact title.
    page: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct CreatePage {
    title: String,
    /// Initial markdown content.
    markdown: Option<String>,
    /// Id or title of the parent page, to nest it like a folder.
    parent: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct AppendToPage {
    page: String,
    markdown: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct ReplaceInPage {
    page: String,
    /// Exact text to find. Must appear exactly once.
    find: String,
    replace: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct Search {
    query: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct WaitForMessages {
    /// Only wake for this channel (name or id). Omit to watch all channels.
    channel: Option<String>,
    /// Give up after this many seconds (max 300). Defaults to 120.
    timeout_seconds: Option<u64>,
}

/// One line per message, oldest first, each starting with its id.
fn format_messages(messages: Vec<Message>) -> String {
    if messages.is_empty() {
        return "(no messages yet)".into();
    }
    let mut output = String::new();
    for message in messages {
        let bot = if message.author_is_bot { " (bot)" } else { "" };
        let reply = message.reply_to.map(|id| format!(" ↩{id}")).unwrap_or_default();
        let author = match message.author_name.as_str() {
            "" => format!("member {}", &message.author[..8]),
            name => name.to_owned(),
        };
        output.push_str(&format!(
            "[{}] {author}{bot}{reply}: {}",
            message.id, message.text
        ));
        for reaction in message.reactions {
            output.push_str(&format!(" {}×{}", reaction.emoji, reaction.count));
        }
        output.push('\n');
    }
    output
}

fn failed(error: anyhow::Error) -> String {
    format!("{error:#}")
}

impl ShyTools {
    fn channel_id(&self, channel: &str) -> Result<String> {
        let wanted = channel.trim_start_matches('#');
        self.workspace
            .channels()?
            .into_iter()
            .find(|candidate| candidate.id == wanted || candidate.name == wanted)
            .map(|found| found.id)
            .with_context(|| format!("no channel named {channel}; call list_channels"))
    }

    fn page_id(&self, page: &str) -> Result<String> {
        self.workspace
            .pages()?
            .into_iter()
            .find(|candidate| candidate.id == page || candidate.title == page)
            .map(|found| found.id)
            .with_context(|| format!("no page {page}; call list_pages"))
    }
}

#[tool_router]
impl ShyTools {
    pub fn new(workspace: Workspace) -> Self {
        Self { workspace }
    }

    #[tool(description = "List channels in the workspace.")]
    fn list_channels(&self) -> ToolResult {
        let channels = self.workspace.channels().map_err(failed)?;
        Ok(channels
            .iter()
            .map(|channel| format!("#{} ({})", channel.name, channel.id))
            .collect::<Vec<_>>()
            .join("\n"))
    }

    #[tool(
        description = "Read recent messages in a channel, oldest first. Each line starts with the message id."
    )]
    fn read_messages(&self, Parameters(request): Parameters<ReadMessages>) -> ToolResult {
        let channel = self.channel_id(&request.channel).map_err(failed)?;
        let limit = request.limit.unwrap_or(DEFAULT_MESSAGE_LIMIT);
        let messages = match &request.before {
            Some(message) => self.workspace.messages_before(&channel, message, limit),
            None => self.workspace.messages(&channel, None, limit),
        };
        messages.map(format_messages).map_err(failed)
    }

    #[tool(description = "Post a message to a channel, optionally as a reply.")]
    fn send_message(&self, Parameters(request): Parameters<SendMessage>) -> ToolResult {
        let channel = self.channel_id(&request.channel).map_err(failed)?;
        let id = self
            .workspace
            .send_message(&channel, &request.text, request.reply_to)
            .map_err(failed)?;
        Ok(format!("sent message {id}"))
    }

    #[tool(description = "Add an emoji reaction to a message.")]
    fn react(&self, Parameters(request): Parameters<React>) -> ToolResult {
        self.workspace
            .react(&request.message_id, &request.emoji, true)
            .map_err(failed)?;
        Ok("reacted".into())
    }

    #[tool(description = "List pages (collaborative markdown documents) with their ids and parents.")]
    fn list_pages(&self) -> ToolResult {
        let pages = self.workspace.pages().map_err(failed)?;
        Ok(pages
            .iter()
            .map(|page| match &page.parent {
                Some(parent) => format!("{} ({}) in {parent}", page.title, page.id),
                None => format!("{} ({})", page.title, page.id),
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    #[tool(description = "Read a page's markdown.")]
    fn read_page(&self, Parameters(request): Parameters<PageRef>) -> ToolResult {
        let page = self.page_id(&request.page).map_err(failed)?;
        self.workspace.page_text(&page).map_err(failed)
    }

    #[tool(description = "Create a page. Returns its id; share it in chat as shy://page/<id>.")]
    fn create_page(&self, Parameters(request): Parameters<CreatePage>) -> ToolResult {
        let parent = request
            .parent
            .map(|parent| self.page_id(&parent))
            .transpose()
            .map_err(failed)?;
        let id = self
            .workspace
            .create_page(&request.title, parent)
            .map_err(failed)?;
        if let Some(markdown) = request.markdown.filter(|markdown| !markdown.is_empty()) {
            self.workspace
                .edit_page(&id, Edit::Append(&markdown))
                .map_err(failed)?;
        }
        Ok(format!("created page {id}"))
    }

    #[tool(description = "Append markdown to the end of a page. Safe while humans are editing it.")]
    fn append_to_page(&self, Parameters(request): Parameters<AppendToPage>) -> ToolResult {
        let page = self.page_id(&request.page).map_err(failed)?;
        self.workspace
            .edit_page(&page, Edit::Append(&request.markdown))
            .map_err(failed)?;
        Ok("appended".into())
    }

    #[tool(
        description = "Replace one exact, unique passage in a page. Edits only that passage, so concurrent human edits elsewhere are kept."
    )]
    fn replace_in_page(&self, Parameters(request): Parameters<ReplaceInPage>) -> ToolResult {
        let page = self.page_id(&request.page).map_err(failed)?;
        self.workspace
            .edit_page(
                &page,
                Edit::Replace {
                    find: &request.find,
                    with: &request.replace,
                },
            )
            .map_err(failed)?;
        Ok("replaced".into())
    }

    #[tool(description = "Full-text search across messages and pages.")]
    fn search(&self, Parameters(request): Parameters<Search>) -> ToolResult {
        let hits = self.workspace.search(&request.query, 20).map_err(failed)?;
        if hits.is_empty() {
            return Ok("no results".into());
        }
        Ok(hits
            .iter()
            .map(|hit| {
                let snippet = hit.snippet.replace(['\u{1}', '\u{2}'], "**");
                format!("{} {} \"{}\": {snippet}", hit.kind, hit.id, hit.title)
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    #[tool(
        description = "Block until someone else posts a message (optionally in one channel), then return that channel's recent messages. Use it to stay in a conversation."
    )]
    async fn wait_for_messages(&self, Parameters(request): Parameters<WaitForMessages>) -> ToolResult {
        let wanted = request
            .channel
            .map(|channel| self.channel_id(&channel))
            .transpose()
            .map_err(failed)?;
        let timeout = Duration::from_secs(request.timeout_seconds.unwrap_or(120).min(MAX_WAIT_SECONDS));
        let me = self.workspace.my_id();
        let mut changes = self.workspace.subscribe();
        let arrived = tokio::time::timeout(timeout, async {
            while let Ok(change) = changes.recv().await {
                let Change::Messages { channel } = change else {
                    continue;
                };
                if wanted.as_ref().is_some_and(|wanted| *wanted != channel) {
                    continue;
                }
                let latest = self.workspace.messages(&channel, None, 1).ok()?;
                if latest.first().is_some_and(|message| message.author != me) {
                    return Some(channel);
                }
            }
            None
        })
        .await;
        match arrived {
            Ok(Some(channel)) => self
                .workspace
                .messages(&channel, None, 10)
                .map(format_messages)
                .map_err(failed),
            _ => Ok("no new messages".into()),
        }
    }
}

#[tool_handler]
impl ServerHandler for ShyTools {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(format!(
            "You are a member of the shyspace workspace \"{}\".\n\n{}",
            self.workspace.name(),
            crate::skill::guidance()
        ))
    }
}

pub async fn serve(workspace: Workspace) -> Result<()> {
    let service = ShyTools::new(workspace.clone()).serve(stdio()).await?;
    service.waiting().await?;
    workspace.shutdown().await;
    Ok(())
}
