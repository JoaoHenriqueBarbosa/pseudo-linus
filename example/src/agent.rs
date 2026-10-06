//! O agente: `ClaudeSDKClient` do prana sobre o `NativeApiTransport` (laço in-process, sem o binário
//! do Claude), com as tools embutidas desligadas e só o `osh` disponível.

use std::collections::HashMap;

use anyhow::{bail, Result};
use prana::{
    ClaudeAgentOptions, ClaudeSDKClient, ContentBlock, Message, NativeApiTransport, PermissionMode, SystemPromptConfig,
    ToolsConfig,
};
use serde_json::Value;

use crate::cassette::Cassette;
use crate::osh_tool;
use crate::sandbox::Sandbox;

pub const DEFAULT_MODEL: &str = "claude-sonnet-4-5";

const SYSTEM_PROMPT: &str = "You are operating a Linux machine (Debian 13) through a single tool, `osh`, \
which runs bash commands in persistent shell sessions. Do the task completely, verify the result with real \
commands, and finish with a short plain-text answer stating what you found or did.";

/// Uma chamada de tool feita pelo modelo, com o resultado que voltou.
#[derive(Clone, Debug)]
pub struct ToolCall {
    pub id: String,
    pub input: Value,
    pub output: String,
    pub is_error: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Transcript {
    pub final_text: String,
    pub tool_calls: Vec<ToolCall>,
    pub is_error: bool,
    pub num_turns: i64,
}

impl Transcript {
    /// Os comandos que o modelo mandou para o `osh`, em ordem.
    pub fn commands(&self) -> Vec<String> {
        self.tool_calls.iter().filter_map(|c| c.input["command"].as_str().map(str::to_string)).collect()
    }

    /// As sessões que o modelo usou.
    pub fn sessions(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .tool_calls
            .iter()
            .map(|c| c.input["session"].as_str().unwrap_or("main").to_string())
            .collect();
        v.sort();
        v.dedup();
        v
    }
}

/// Roda um agente até o fim no canal `channel` da fita.
pub async fn run(sandbox: &Sandbox, cassette: &Cassette, channel: &str, prompt: &str) -> Result<Transcript> {
    let transport = NativeApiTransport::new(options(sandbox, cassette, channel));
    let mut client = ClaudeSDKClient::new(options(sandbox, cassette, channel)).with_transport(Box::new(transport));
    client.connect().await?;
    client.query(prompt).await?;
    let messages = client.receive_response().await?;
    client.disconnect().await?;
    transcript(messages)
}

/// As opções do agente; montadas duas vezes porque o transporte e o cliente ficam cada um com as suas.
fn options(sandbox: &Sandbox, cassette: &Cassette, channel: &str) -> ClaudeAgentOptions {
    let mut env = HashMap::new();
    env.insert("ANTHROPIC_BASE_URL".to_string(), cassette.base_url(channel));
    env.insert("ANTHROPIC_API_KEY".to_string(), cassette.api_key());
    let mut opts = ClaudeAgentOptions::default().with_sdk_mcp_server(osh_tool::server(sandbox.clone()));
    opts.tools = Some(ToolsConfig::List(vec![]));
    opts.allowed_tools = vec![osh_tool::qualified_name()];
    opts.system_prompt = Some(SystemPromptConfig::String(SYSTEM_PROMPT.to_string()));
    opts.permission_mode = Some(PermissionMode::BypassPermissions);
    opts.max_turns = Some(30);
    opts.model = Some(std::env::var("PL_AGENT_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string()));
    opts.env = env;
    opts
}

fn result_text(c: Option<&prana::ToolResultContent>) -> String {
    match c {
        Some(prana::ToolResultContent::Text(s)) => s.clone(),
        Some(prana::ToolResultContent::Blocks(b)) => {
            b.iter().filter_map(|v| v["text"].as_str()).collect::<Vec<_>>().join("\n")
        }
        None => String::new(),
    }
}

fn transcript(messages: Vec<Message>) -> Result<Transcript> {
    let mut t = Transcript::default();
    let mut by_id: HashMap<String, usize> = HashMap::new();
    let mut saw_result = false;
    for m in messages {
        match m {
            Message::Assistant(a) => {
                for b in a.content {
                    if let ContentBlock::ToolUse(u) = b {
                        by_id.insert(u.id.clone(), t.tool_calls.len());
                        t.tool_calls.push(ToolCall { id: u.id, input: u.input, output: String::new(), is_error: false });
                    }
                }
            }
            Message::User(u) => {
                let prana::MessageContent::Blocks(blocks) = u.content else { continue };
                for b in blocks {
                    let ContentBlock::ToolResult(r) = b else { continue };
                    if let Some(&i) = by_id.get(&r.tool_use_id) {
                        t.tool_calls[i].output = result_text(r.content.as_ref());
                        t.tool_calls[i].is_error = r.is_error.unwrap_or(false);
                    }
                }
            }
            Message::Result(r) => {
                saw_result = true;
                t.final_text = r.result.unwrap_or_default();
                t.is_error = r.is_error;
                t.num_turns = r.num_turns;
            }
            _ => {}
        }
    }
    if !saw_result {
        bail!("o agente terminou sem mensagem de resultado");
    }
    Ok(t)
}
