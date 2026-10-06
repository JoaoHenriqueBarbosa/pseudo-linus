//! A única tool do agente: `osh`, um shell no sandbox. Cada `session` nomeada é um shell próprio,
//! com cwd, variáveis e jobs em segundo plano que sobrevivem entre chamadas.

use prana::{PropertySchema, SdkMcpServer, ToolError, ToolInputSchema, ToolOutput};
use serde::Deserialize;

use crate::sandbox::Sandbox;

pub const SERVER_NAME: &str = "sandbox";
pub const TOOL_NAME: &str = "osh";

/// O nome qualificado com que o modelo vê a tool.
pub fn qualified_name() -> String {
    format!("mcp__{SERVER_NAME}__{TOOL_NAME}")
}

#[derive(Deserialize)]
struct OshArgs {
    command: String,
    #[serde(default)]
    session: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

const DESCRIPTION: &str = "Run a bash command inside the Linux sandbox. Each `session` is a separate, \
persistent shell: cwd, environment variables and background jobs (`cmd &`) survive between calls of the \
same session. Use different session names to run things concurrently (for example a server in one session \
and a client in another). Returns stdout, stderr and the exit code.";

pub fn server(sandbox: Sandbox) -> SdkMcpServer {
    SdkMcpServer::builder(SERVER_NAME)
        .tool(
            TOOL_NAME,
            DESCRIPTION,
            ToolInputSchema::object()
                .required("command", PropertySchema::string().description("The bash command line to run."))
                .optional(
                    "session",
                    PropertySchema::string().description("Shell session name. Defaults to \"main\"."),
                )
                .optional(
                    "timeout_ms",
                    PropertySchema::integer().description("Time limit for this command in milliseconds."),
                ),
            move |args: OshArgs| {
                let sandbox = sandbox.clone();
                async move {
                    let session = args.session.as_deref().unwrap_or("main");
                    let out = sandbox.exec(session, &args.command, args.timeout_ms).await.map_err(ToolError::from_error)?;
                    Ok(ToolOutput::text(render(&out)))
                }
            },
        )
        .build()
}

fn render(out: &crate::sandbox::ExecOutput) -> String {
    let mut s = String::new();
    if !out.stdout.is_empty() {
        s.push_str(&out.stdout);
        if !out.stdout.ends_with('\n') {
            s.push('\n');
        }
    }
    if !out.stderr.is_empty() {
        s.push_str("[stderr]\n");
        s.push_str(&out.stderr);
        if !out.stderr.ends_with('\n') {
            s.push('\n');
        }
    }
    match out.exit_code {
        Some(c) => s.push_str(&format!("[exit {c}]")),
        None => s.push_str("[no exit code]"),
    }
    if out.timed_out {
        s.push_str(" [timed out]");
    }
    if out.session_reset {
        s.push_str(" [session was reset]");
    }
    s
}
