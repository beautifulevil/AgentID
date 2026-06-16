mod api;
mod commands;
mod config;
mod constants;
mod github;
mod help;
mod keychain;
mod ui;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "agentid",
    version,
    about = "Git identity for AI agents",
    disable_help_subcommand = true
)]
struct Cli {
    #[arg(
        long,
        env = "AGENTID_API_BASE_URL",
        default_value = "https://api.agentid.beautifulevilcompany.com",
        hide = true
    )]
    api_base_url: String,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    Login {
        #[command(subcommand)]
        command: Option<LoginCommand>,
    },
    Logout,
    Status,
    Init {
        path: String,
    },
    Scan,
    Workspace,
    Orgs,
    Org {
        name: Option<String>,
    },
    Agents,
    New {
        name: Option<String>,
        #[arg(long)]
        display: Option<String>,
    },
    Allow {
        agent: Option<String>,
        #[arg(long)]
        repos: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long, default_value = "dev,feature/*")]
        branches: String,
    },
    Use {
        agent: Option<String>,
    },
    Github,
    Revoke,
    Settings,
    Help,
    #[command(hide = true, name = "git-credential")]
    GitCredential {
        operation: Option<String>,
    },
}

#[derive(Subcommand)]
enum LoginCommand {
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let ctx = commands::Context::new(cli.api_base_url);

    match cli.command {
        None => commands::interactive_menu(ctx).await,
        Some(Command::Login { command }) => match command {
            None => commands::login(ctx).await,
            Some(LoginCommand::Status) => commands::login_status(ctx).await,
        },
        Some(Command::Logout) => commands::logout(),
        Some(Command::Status) => commands::status(ctx).await,
        Some(Command::Init { path }) => commands::workspace_init(ctx, &path).await,
        Some(Command::Scan) => commands::workspace_scan(&ctx),
        Some(Command::Workspace) => commands::project_menu(ctx).await,
        Some(Command::Orgs) => commands::org_list(ctx).await,
        Some(Command::Org { name }) => match name {
            Some(name) => commands::org_select(ctx, &name).await,
            None => commands::org_list(ctx).await,
        },
        Some(Command::Agents) => commands::agent_list(ctx).await,
        Some(Command::New { name, display }) => match (name, display) {
            (Some(name), Some(display)) => commands::agent_create(ctx, &name, &display).await,
            _ => commands::agent_create_interactive(ctx).await,
        },
        Some(Command::Allow {
            agent,
            repos,
            all,
            branches,
        }) => {
            if agent.is_none() && repos.is_none() && !all {
                commands::allow_interactive(ctx).await
            } else {
                let agent = agent.ok_or_else(|| anyhow::anyhow!("agent name required"))?;
                commands::allow(ctx, &agent, repos.as_deref(), all, &branches).await
            }
        }
        Some(Command::Use { agent }) => commands::use_agent(ctx, agent.as_deref()).await,
        Some(Command::Github) => commands::github_install(ctx).await,
        Some(Command::Revoke) => commands::device_revoke(ctx).await,
        Some(Command::Settings) => commands::settings_menu(ctx).await,
        Some(Command::Help) => {
            help::print_help();
            Ok(())
        }
        Some(Command::GitCredential { operation }) => {
            commands::git_credential(ctx, operation.as_deref()).await
        }
    }
}
