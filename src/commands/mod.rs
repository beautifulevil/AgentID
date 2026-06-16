use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::json;
use tokio::time::sleep;

use crate::api::{
    AgentListResponse, ApiClient, DeleteAccountResponse, DeleteAgentResponse, DevicePollResponse,
    DeviceStartResponse, DisconnectOrgResponse, EmailStartResponse, EmailVerifyResponse,
    GitHubTokenResponse, InstallResponse, MeResponse, OrgListResponse, PermissionListResponse,
};
use crate::config::{self, GlobalConfig};
use crate::constants::BOT_COMMIT_EMAIL;
use crate::github;
use crate::keychain::{self, Session};
use crate::ui;

pub struct Context {
    api_base_url: String,
    menu_mode: bool,
    suppress_screen: bool,
}

impl Clone for Context {
    fn clone(&self) -> Self {
        Self {
            api_base_url: self.api_base_url.clone(),
            menu_mode: self.menu_mode,
            suppress_screen: self.suppress_screen,
        }
    }
}

impl Context {
    pub fn new(api_base_url: String) -> Self {
        Self {
            api_base_url,
            menu_mode: false,
            suppress_screen: false,
        }
    }

    pub fn for_menu(&self) -> Self {
        Self {
            menu_mode: true,
            ..self.clone()
        }
    }

    pub fn without_screen(&self) -> Self {
        Self {
            suppress_screen: true,
            ..self.clone()
        }
    }

    fn api(&self) -> ApiClient {
        ApiClient::new(self.api_base_url.clone())
    }
}

fn maybe_heading(ctx: &Context, title: &str) {
    if !ctx.menu_mode {
        ui::heading(title);
    }
}

fn action_screen(ctx: &Context) -> Result<()> {
    if ctx.menu_mode && !ctx.suppress_screen {
        let session = keychain::load_session()?;
        ui::begin_action_screen(&session.user_email);
    }
    Ok(())
}

pub async fn interactive_menu(ctx: Context) -> Result<()> {
    let menu_ctx = ctx.for_menu();
    let mut first_menu = true;

    loop {
        ensure_authenticated(&menu_ctx).await?;
        let session = keychain::load_session()?;
        ui::begin_menu_screen(&session.user_email, first_menu);
        first_menu = false;

        let sections = [
            ui::MenuSection {
                title: "Setup",
                hint: "wizard & status",
                items: &["Setup wizard", "View status"],
            },
            ui::MenuSection {
                title: "GitHub",
                hint: "orgs & bots",
                items: &["Organizations", "Bots"],
            },
            ui::MenuSection {
                title: "Project",
                hint: "this folder",
                items: &[
                    "Initialize this folder",
                    "Scan repositories",
                    "Grant repo access",
                    "Activate bot for git",
                    "Manage workspaces",
                ],
            },
            ui::MenuSection {
                title: "Account",
                hint: "help & sign out",
                items: &["Help", "Account & security", "Log out", "Exit"],
            },
        ];

        let choice = ui::select_sectioned_menu(&sections)?;
        let result = match choice {
            0 => guided_setup(menu_ctx.clone()).await,
            1 => status(menu_ctx.clone()).await,
            2 => organizations_menu(menu_ctx.clone()).await,
            3 => agents_menu(menu_ctx.clone()).await,
            4 => project_action(menu_ctx.clone(), ProjectAction::Init).await,
            5 => project_action(menu_ctx.clone(), ProjectAction::Scan).await,
            6 => project_action(menu_ctx.clone(), ProjectAction::Allow).await,
            7 => project_action(menu_ctx.clone(), ProjectAction::Use).await,
            8 => workspace_menu(menu_ctx.clone()).await,
            9 => {
                crate::help::print_help();
                Ok(())
            }
            10 => account_menu(menu_ctx.clone()).await,
            11 => logout(),
            12 => Ok(()),
            _ => Ok(()),
        };
        result?;

        if choice == 12 {
            ui::success("See you next time.");
            break;
        }
    }
    Ok(())
}

async fn guided_setup(ctx: Context) -> Result<()> {
    action_screen(&ctx)?;
    let ctx = ctx.without_screen();

    println!("  {}", ui::wizard_title());
    ui::explain(
        "Walk through AgentID one step at a time.\n\
         Each step explains what it does. Type y or n when prompted.",
    );

    // Step 1 — GitHub org
    ui::step(1, "GitHub organization");
    ui::explain(
        "AgentID uses a GitHub App installed on your organization.\n\
         The active org must match the owner in `git remote` (e.g. `dodeys/repo` → org `dodeys`).\n\
         That org is used for listing bots and for `git push` credentials.",
    );
    let orgs: OrgListResponse = ctx
        .api()
        .get("/orgs", Some(&keychain::load_session()?))
        .await?;
    if orgs.orgs.is_empty() {
        ui::info("No GitHub organization is connected yet.");
        if ui::confirm_step("Connect a GitHub organization now?")? {
            github_install(ctx.clone()).await?;
        } else {
            ui::info("Skipped. You can connect an org anytime from the menu.");
            return Ok(());
        }
    } else {
        let rows: Vec<Vec<String>> = orgs
            .orgs
            .iter()
            .map(|org| {
                let selected = orgs.selected_org.as_deref() == Some(org.org.as_str());
                let marker = if selected { " ← active" } else { "" };
                vec![format!("{}{}", org.org, marker), org.account_login.clone()]
            })
            .collect();
        ui::print_table(&["ORG", "ACCOUNT"], &rows);
        if let Some(remote_owner) = github::current_remote_owner() {
            ui::explain(&format!(
                "This folder's git remote owner is `{remote_owner}`.\n\
                 Pick that org as active so push and permissions stay consistent."
            ));
        }
        if ui::confirm_step("Choose which organization to use?")? {
            guided_pick_org(&ctx, &orgs).await?;
        } else {
            ui::info("Keeping current organization selection.");
        }
    }

    // Step 2 — Bot
    ui::step(2, "Register a bot");
    ui::explain(
        "A bot is the git identity for your AI agent — a display name on commits.\n\
         Example: `cursor` → Cursor Agent Bot",
    );
    let agents: AgentListResponse = ctx
        .api()
        .get("/agents", Some(&keychain::load_session()?))
        .await?;
    if agents.agents.is_empty() {
        if ui::confirm_step("Register your first bot now?")? {
            register_new_agent(&ctx).await?;
        } else {
            ui::info("Skipped bot registration.");
        }
    } else {
        let rows: Vec<Vec<String>> = agents
            .agents
            .iter()
            .map(|agent| {
                vec![
                    agents.org.clone(),
                    agent.display_name.clone(),
                    agent.name.clone(),
                ]
            })
            .collect();
        ui::print_table(&["ORG", "BOT NAME", "AGENT"], &rows);
        if ui::confirm_step("Register another bot?")? {
            register_new_agent(&ctx).await?;
        }
    }

    // Step 3 — Workspace
    ui::step(3, "Workspace file");
    ui::explain(
        "Creates `.agentid/workspace.yaml` in this folder.\n\
         It records which org and active bot this project uses for git operations.",
    );
    if config::load_workspace().is_err() {
        if ui::confirm_step("Initialize workspace in this folder?")? {
            workspace_init(ctx.clone(), ".").await?;
        } else {
            ui::info("Skipped workspace init.");
        }
    } else {
        ui::info("Workspace already exists in this folder — skipped.");
    }

    // Step 4 — Scan
    ui::step(4, "Detect repositories");
    ui::explain(
        "Scans this folder for git repositories.\n\
         Needed before you can grant a bot push access to specific repos.",
    );
    if ui::confirm_step("Scan for git repositories here?")? {
        workspace_scan(&ctx)?;
    }

    // Step 5 — Permissions
    ui::step(5, "Repository permissions");
    ui::explain(
        "Grants a bot permission to push to selected repos and branches.\n\
         Without this, `git push` will be denied even if the bot is active.",
    );
    if ui::confirm_step("Grant repository access to a bot?")? {
        allow_interactive(ctx.clone()).await?;
    }

    // Step 6 — Activate
    ui::step(6, "Activate bot for git");
    ui::explain(
        "Sets local git user.name and connects `git push` to AgentID.\n\
         Commits in this folder will use the bot identity you choose.",
    );
    if ui::confirm_step("Activate a bot for git in this folder?")? {
        use_agent(ctx.clone(), None).await?;
    }

    ui::success("Wizard finished. Try `git commit` and `git push` when you're ready.");
    Ok(())
}

async fn guided_pick_org(ctx: &Context, response: &OrgListResponse) -> Result<()> {
    if response.orgs.is_empty() {
        return Err(anyhow!("no organizations connected"));
    }

    if let Some(remote_owner) = github::current_remote_owner() {
        if let Some(match_org) = response
            .orgs
            .iter()
            .find(|org| github::org_matches_remote(&org.org, &remote_owner))
        {
            if ui::confirm_step(&format!(
                "Use `{0}` (matches git remote `{remote_owner}`)?",
                match_org.org
            ))? {
                return org_select(ctx.clone(), &match_org.org).await;
            }
        }
    }

    let options: Vec<String> = response
        .orgs
        .iter()
        .map(|org| {
            if response.selected_org.as_deref() == Some(org.org.as_str()) {
                format!("{} (current)", org.org)
            } else {
                org.org.clone()
            }
        })
        .collect();
    let labels: Vec<&str> = options.iter().map(String::as_str).collect();
    let choice = ui::select_option("Active organization", &labels)?;
    org_select(ctx.clone(), &response.orgs[choice].org).await
}

async fn pick_agent(ctx: &Context, org: &str) -> Result<String> {
    let session = keychain::load_session()?;
    let response: AgentListResponse = ctx
        .api()
        .get(&format!("/agents?org={}", url_encode(org)), Some(&session))
        .await?;
    if response.agents.is_empty() {
        return Err(anyhow!(
            "no bots registered for {org} yet — use GitHub → Bots"
        ));
    }
    let options: Vec<String> = response
        .agents
        .iter()
        .map(|agent| format!("{} ({})", agent.display_name, agent.name))
        .collect();
    let labels: Vec<&str> = options.iter().map(String::as_str).collect();
    let choice = ui::select_option("Choose a bot", &labels)?;
    Ok(response.agents[choice].name.clone())
}

pub async fn allow_interactive(ctx: Context) -> Result<()> {
    if !ctx.menu_mode {
        action_screen(&ctx)?;
    }
    let workspace = require_connected_workspace_org(&ctx).await?;
    let agent = pick_agent(&ctx, &workspace.org).await?;

    let mut repos = workspace.detected_repos.clone();
    if repos.is_empty() {
        ui::info("No repos scanned yet — scanning now.");
        repos = github::scan_repositories(&workspace.workspace_path)?;
    }
    if repos.is_empty() {
        return Err(anyhow!(
            "no git repositories found — run `agentid scan` first"
        ));
    }

    let use_all = if repos.len() == 1 {
        ui::confirm_step(&format!("Allow access to `{}`?", repos[0]))?
    } else {
        !ui::confirm_step("Pick specific repositories?")?
    };

    let selected_repos = if use_all {
        repos.clone()
    } else {
        let picked = ui::multi_select("Repositories this bot can use", &repos)?;
        picked
            .into_iter()
            .map(|index| repos[index].clone())
            .collect()
    };

    let branches = ui::input("Allowed branches (comma-separated, e.g. dev,feature/*)")?;
    allow(
        ctx,
        &agent,
        Some(&selected_repos.join(",")),
        false,
        &branches,
    )
    .await
}

pub async fn ensure_authenticated(ctx: &Context) -> Result<Session> {
    if let Ok(session) = keychain::load_session() {
        if ctx
            .api()
            .get::<MeResponse>("/auth/me", Some(&session))
            .await
            .is_ok()
        {
            return Ok(session);
        }
        let _ = keychain::delete_session();
        ui::info("Your session expired. Sign in again to continue.");
    } else {
        ui::info("Welcome to AgentID. Sign in or create an account to continue.");
    }

    login(ctx.clone()).await?;
    keychain::load_session()
}

pub async fn login(ctx: Context) -> Result<()> {
    let api = ctx.api();
    let device_name = hostname::get()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|_| "Unknown device".to_string());
    let start: DeviceStartResponse = api
        .post(
            "/cli/device/start",
            &json!({ "device_name": device_name }),
            None,
        )
        .await?;

    ui::heading("Sign in or create account");
    ui::info("Enter your email. We'll send a one-time code — works for new and existing accounts.");
    let email = ui::input("Email")?;
    let email_start: EmailStartResponse = api
        .post(
            "/auth/email/start",
            &json!({
                "email": email,
                "cli_pairing_code": start.cli_pairing_code
            }),
            None,
        )
        .await?;

    ui::success(&format!("Code sent to {}", email_start.masked_email));
    let email_code = ui::input("Enter the 6-digit code")?;
    let _: EmailVerifyResponse = api
        .post(
            "/auth/email/verify",
            &json!({
                "request_id": email_start.request_id,
                "email_code": email_code
            }),
            None,
        )
        .await?;

    let spinner = ui::spinner("Confirming login…");
    let poll = wait_for_session(&api, &start.device_code, &start.device_id).await?;
    spinner.finish_and_clear();
    let session = Session {
        session_token: required(poll.session_token, "session_token")?,
        refresh_token: required(poll.refresh_token, "refresh_token")?,
        device_id: poll.device_id.unwrap_or(start.device_id),
        user_email: required(poll.user, "user")?,
        expires_at: required(poll.expires_at, "expires_at")?,
    };
    keychain::save_session(&session)?;
    let mut config = config::load_global().unwrap_or_default();
    config.api_base_url = Some(ctx.api_base_url);
    config::save_global(&config)?;
    let _: serde_json::Value = api
        .post(
            "/cli/device/complete",
            &json!({ "device_code": start.device_code }),
            None,
        )
        .await?;

    ui::success(&format!("Signed in as {}", session.user_email));
    Ok(())
}

async fn wait_for_session(
    api: &ApiClient,
    device_code: &str,
    device_id: &str,
) -> Result<DevicePollResponse> {
    for _ in 0..15 {
        let poll: DevicePollResponse = api
            .get(
                &format!("/cli/device/poll?device_code={}", device_code),
                None,
            )
            .await?;
        match poll.status.as_str() {
            "approved" => return Ok(poll),
            "expired" => return Err(anyhow!("login expired; run `agentid login` again")),
            "pending" => sleep(Duration::from_millis(500)).await,
            other => return Err(anyhow!("login failed: {}", other)),
        }
    }
    Err(anyhow!("login timed out for device `{}`", device_id))
}

pub fn logout() -> Result<()> {
    keychain::delete_session()?;
    ui::success("Signed out.");
    Ok(())
}

pub async fn login_status(ctx: Context) -> Result<()> {
    ui::heading("Session");
    let session = match keychain::load_session() {
        Ok(session) => session,
        Err(_) => {
            ui::info("Not signed in. Run `agentid login` or just `agentid`.");
            return Ok(());
        }
    };

    let mut lines = vec![("Account", session.user_email.clone())];
    match ctx
        .api()
        .get::<MeResponse>("/auth/me", Some(&session))
        .await
    {
        Ok(me) => {
            lines.push(("Session", "valid".to_string()));
            lines.push(("Device", me.device_id));
        }
        Err(error) => lines.push(("Session", format!("invalid ({error})"))),
    }

    let orgs = ctx
        .api()
        .get::<OrgListResponse>("/orgs", Some(&session))
        .await
        .ok();
    if let Some(selected) = orgs
        .as_ref()
        .and_then(|response| response.selected_org.as_deref())
    {
        lines.push((
            "Organization",
            org_connection_label(selected, orgs.as_ref()),
        ));
    }
    ui::status_panel(&lines);
    Ok(())
}

pub async fn github_install(ctx: Context) -> Result<()> {
    action_screen(&ctx)?;
    maybe_heading(&ctx, "GitHub organizations");
    let session = keychain::load_session()?;
    let installed = ctx
        .api()
        .get::<OrgListResponse>("/orgs", Some(&session))
        .await?;
    let installed_orgs: std::collections::BTreeSet<String> =
        installed.orgs.iter().map(|org| org.org.clone()).collect();

    let user_orgs = github::list_user_organizations()?;
    let rows: Vec<Vec<String>> = user_orgs
        .iter()
        .map(|org| {
            let status = if installed_orgs.contains(org) {
                "✓ connected".to_string()
            } else {
                "not connected".to_string()
            };
            vec![org.clone(), status]
        })
        .collect();
    ui::print_table(&["ORG", "STATUS"], &rows);
    ui::blank();

    let options: Vec<String> = user_orgs
        .iter()
        .map(|org| {
            if installed_orgs.contains(org) {
                format!("{org} (already connected)")
            } else {
                format!("Connect {org}")
            }
        })
        .collect();
    let labels: Vec<&str> = options.iter().map(String::as_str).collect();
    let choice = ui::select_option("Choose an organization", &labels)?;
    let org_name = &user_orgs[choice];

    if installed_orgs.contains(org_name) {
        let choice = ui::select_option(
            &format!("{org_name} is already connected"),
            &[
                "Use as active organization",
                "Disconnect from AgentID",
                "Cancel",
            ],
        )?;
        match choice {
            0 => {
                org_select(ctx, org_name).await?;
                ui::success(&format!("{org_name} is now the active organization."));
            }
            1 => disconnect_org(&ctx, org_name).await?,
            _ => ui::info("Cancelled."),
        }
        return Ok(());
    }

    install_organization(&ctx, org_name).await
}

pub async fn organizations_menu(ctx: Context) -> Result<()> {
    loop {
        action_screen(&ctx)?;
        maybe_heading(&ctx, "Organizations");
        ui::explain(
            "Connect the AgentID GitHub App to an org, pick which org is active,\n\
             or disconnect and uninstall the app from an org.",
        );

        let session = keychain::load_session()?;
        let installed = ctx
            .api()
            .get::<OrgListResponse>("/orgs", Some(&session))
            .await?;

        if installed.orgs.is_empty() {
            ui::info("No organizations connected yet.");
        } else {
            let rows: Vec<Vec<String>> = installed
                .orgs
                .iter()
                .map(|org| {
                    let selected = installed.selected_org.as_deref() == Some(org.org.as_str());
                    let marker = if selected { " ← active" } else { "" };
                    vec![format!("{}{}", org.org, marker), org.account_login.clone()]
                })
                .collect();
            ui::print_table(&["ORG", "ACCOUNT"], &rows);
            ui::blank();
        }

        if let Ok(user_orgs) = github::list_user_organizations() {
            if !user_orgs.is_empty() {
                let connected: std::collections::BTreeSet<String> =
                    installed.orgs.iter().map(|org| org.org.clone()).collect();
                let rows: Vec<Vec<String>> = user_orgs
                    .iter()
                    .map(|org| {
                        let status = if connected.contains(org) {
                            "connected".to_string()
                        } else {
                            "not connected".to_string()
                        };
                        vec![org.clone(), status]
                    })
                    .collect();
                ui::print_table(&["YOUR GITHUB ORGS", "AGENTID"], &rows);
                ui::blank();
            }
        }

        let mut menu_items = vec!["Connect an organization"];
        if !installed.orgs.is_empty() {
            menu_items.push("Set active organization");
            menu_items.push("Disconnect an organization");
        }
        menu_items.push("Back");

        let choice = ui::select_option("Organizations", &menu_items)?;
        match menu_items[choice] {
            "Connect an organization" => github_install(ctx.clone()).await?,
            "Set active organization" => switch_active_org(ctx.clone()).await?,
            "Disconnect an organization" => disconnect_org_interactive(&ctx).await?,
            _ => return Ok(()),
        }
    }
}

async fn switch_active_org(ctx: Context) -> Result<()> {
    let session = keychain::load_session()?;
    let response: OrgListResponse = ctx.api().get("/orgs", Some(&session)).await?;
    if response.orgs.is_empty() {
        ui::info("Connect an organization first.");
        if ui::confirm_step("Connect one now?")? {
            return github_install(ctx).await;
        }
        return Ok(());
    }

    let options: Vec<String> = response
        .orgs
        .iter()
        .map(|org| {
            if response.selected_org.as_deref() == Some(org.org.as_str()) {
                format!("{} (active)", org.org)
            } else {
                org.org.clone()
            }
        })
        .collect();
    let labels: Vec<&str> = options.iter().map(String::as_str).collect();
    let choice = ui::select_option("Set active organization", &labels)?;
    org_select(ctx, &response.orgs[choice].org).await
}

pub async fn org_list(ctx: Context) -> Result<()> {
    organizations_menu(ctx).await
}

async fn install_organization(ctx: &Context, org_name: &str) -> Result<()> {
    let session = keychain::load_session()?;
    let path = format!(
        "/github/install?org={}",
        url::form_urlencoded::byte_serialize(org_name.as_bytes()).collect::<String>()
    );
    let response: InstallResponse = ctx.api().get(&path, Some(&session)).await?;

    ui::info(&format!("Opening GitHub to connect {org_name}…"));
    open::that(&response.install_url)
        .map_err(|error| anyhow!("failed to open browser: {error}"))?;

    let spinner = ui::spinner(&format!(
        "Waiting for {org_name} authorization in the browser…"
    ));
    for _ in 0..40 {
        sleep(Duration::from_secs(3)).await;
        let orgs: OrgListResponse = ctx.api().get("/orgs", Some(&session)).await?;
        if orgs.orgs.iter().any(|org| org.org == org_name) {
            spinner.finish_and_clear();
            org_select(ctx.clone(), org_name).await?;
            ui::success(&format!("{org_name} connected and selected."));
            return Ok(());
        }
    }
    spinner.finish_and_clear();

    Err(anyhow!(
        "Timed out waiting for '{org_name}'. Finish in the browser, then run `agentid orgs`."
    ))
}

pub async fn org_select(ctx: Context, org: &str) -> Result<()> {
    let session = keychain::load_session()?;
    let _: serde_json::Value = ctx
        .api()
        .post("/orgs/select", &json!({ "org": org }), Some(&session))
        .await?;
    let mut config = config::load_global().unwrap_or_default();
    config.selected_org = Some(org.to_lowercase());
    config::save_global(&config)?;
    ui::success(&format!("Active organization: {}", org.to_lowercase()));
    Ok(())
}

pub async fn agent_create(ctx: Context, name: &str, display: &str) -> Result<()> {
    let session = keychain::load_session()?;
    let _: serde_json::Value = ctx
        .api()
        .post(
            "/agents",
            &json!({ "name": name, "display": display }),
            Some(&session),
        )
        .await?;
    ui::success(&format!("Created {display} ({name})"));
    Ok(())
}

pub async fn agent_create_interactive(ctx: Context) -> Result<()> {
    ensure_org_selected(&ctx).await?;
    register_new_agent(&ctx).await
}

pub async fn agents_menu(ctx: Context) -> Result<()> {
    loop {
        action_screen(&ctx)?;
        maybe_heading(&ctx, "Bots");
        ui::explain(
            "Bots are git identities on the server (display name on commits).\n\
             Register them per organization, then grant repo access from Project workflow.",
        );

        let session = keychain::load_session()?;
        let org = match pick_connected_org(&ctx, "Show bots for which organization?").await {
            Ok(org) => org,
            Err(error) => {
                ui::info(&error.to_string());
                return Ok(());
            }
        };

        let response: AgentListResponse = ctx
            .api()
            .get(&format!("/agents?org={}", url_encode(&org)), Some(&session))
            .await?;

        if response.agents.is_empty() {
            ui::info(&format!("No bots registered for {org} yet."));
        } else {
            let rows: Vec<Vec<String>> = response
                .agents
                .iter()
                .map(|agent| {
                    vec![
                        response.org.clone(),
                        agent.display_name.clone(),
                        agent.name.clone(),
                    ]
                })
                .collect();
            ui::print_table(&["ORG", "BOT NAME", "AGENT"], &rows);
            ui::blank();
        }

        let choice = ui::select_option("Bots", &["Register a bot", "Delete a bot", "Back"])?;
        match choice {
            0 => {
                org_select(ctx.clone(), &org).await?;
                register_new_agent(&ctx).await?;
            }
            1 => delete_agent_in_org(&ctx, &org).await?,
            2 => return Ok(()),
            _ => return Ok(()),
        }
    }
}

pub async fn agent_list(ctx: Context) -> Result<()> {
    agents_menu(ctx).await
}

pub async fn account_menu(ctx: Context) -> Result<()> {
    loop {
        action_screen(&ctx)?;
        maybe_heading(&ctx, "Account & security");
        let choice = ui::select_option(
            "Account",
            &["Delete my account", "Sign out this device", "Back"],
        )?;
        match choice {
            0 => delete_account_interactive(&ctx).await?,
            1 => device_revoke(ctx.clone()).await?,
            2 => return Ok(()),
            _ => return Ok(()),
        }
    }
}

pub async fn settings_menu(ctx: Context) -> Result<()> {
    account_menu(ctx).await
}

async fn delete_agent_in_org(ctx: &Context, org: &str) -> Result<()> {
    let session = keychain::load_session()?;
    let response: AgentListResponse = ctx
        .api()
        .get(&format!("/agents?org={}", url_encode(org)), Some(&session))
        .await?;
    if response.agents.is_empty() {
        ui::info(&format!("No bots to delete in {org}."));
        return Ok(());
    }

    let options: Vec<String> = response
        .agents
        .iter()
        .map(|agent| format!("{} ({})", agent.display_name, agent.name))
        .collect();
    let labels: Vec<&str> = options.iter().map(String::as_str).collect();
    let choice = ui::select_option("Delete which bot?", &labels)?;
    let agent = &response.agents[choice];

    if !ui::confirm_step(&format!(
        "Delete {} ({})? Permissions for this bot will be removed.",
        agent.display_name, agent.name
    ))? {
        ui::info("Cancelled.");
        return Ok(());
    }

    let deleted: DeleteAgentResponse = ctx
        .api()
        .delete(
            &format!("/agents/{}?org={}", agent.name, url_encode(org)),
            Some(&session),
        )
        .await?;

    if let Ok(mut workspace) = config::load_workspace() {
        if workspace.active_agent.as_deref() == Some(agent.name.as_str()) {
            workspace.active_agent = None;
            config::save_workspace(&workspace)?;
        }
    }

    ui::success(&format!(
        "Deleted {} from {}.",
        deleted.display, deleted.org
    ));
    Ok(())
}

async fn disconnect_org_interactive(ctx: &Context) -> Result<()> {
    let session = keychain::load_session()?;
    let orgs: OrgListResponse = ctx.api().get("/orgs", Some(&session)).await?;
    if orgs.orgs.is_empty() {
        ui::info("No organizations are connected to AgentID.");
        return Ok(());
    }
    let org = pick_connected_org(ctx, "Disconnect which organization?").await?;
    disconnect_org(ctx, &org).await
}

async fn disconnect_org(ctx: &Context, org: &str) -> Result<()> {
    let session = keychain::load_session()?;

    ui::info(&format!(
        "This removes AgentID from `{org}`: all bots, permissions, and the GitHub App installation."
    ));
    if !ui::confirm_step(&format!("Disconnect `{org}` from AgentID?"))? {
        ui::info("Cancelled.");
        return Ok(());
    }

    let result: DisconnectOrgResponse = ctx
        .api()
        .delete(&format!("/orgs/{org}"), Some(&session))
        .await?;

    let mut global = config::load_global().unwrap_or_default();
    if global.selected_org.as_deref() == Some(org) {
        global.selected_org = None;
        config::save_global(&global)?;
    }

    if result.github_removed {
        ui::success(&format!(
            "Disconnected `{org}` and removed the GitHub App installation."
        ));
    } else {
        ui::success(&format!("Disconnected `{org}` from AgentID."));
        ui::info("If the app is still listed on GitHub, remove it manually:");
        println!("  {}", result.manage_url);
    }
    Ok(())
}

async fn delete_account_interactive(ctx: &Context) -> Result<()> {
    let session = keychain::load_session()?;
    ui::info("This permanently deletes your AgentID account, devices, and bots you created.");

    let typed = ui::input(&format!("Type `{}` to confirm", session.user_email))?;
    if typed.trim().to_lowercase() != session.user_email.to_lowercase() {
        return Err(anyhow!("confirmation email did not match"));
    }
    if !ui::confirm_step("Delete your AgentID account permanently?")? {
        ui::info("Cancelled.");
        return Ok(());
    }

    let _: DeleteAccountResponse = ctx.api().delete("/account", Some(&session)).await?;
    keychain::delete_session()?;
    let mut global = config::load_global().unwrap_or_default();
    global.selected_org = None;
    global.workspace_path = None;
    config::save_global(&global)?;

    ui::success("Account deleted. You can sign in again anytime with `agentid login`.");
    Ok(())
}

async fn ensure_org_selected(ctx: &Context) -> Result<()> {
    let global = config::load_global().unwrap_or_default();
    if global.selected_org.is_some() {
        return Ok(());
    }
    if global.selected_org.is_none() {
        ui::info("Pick an organization first.");
        org_list(ctx.clone()).await?;
    }
    Ok(())
}

async fn register_new_agent(ctx: &Context) -> Result<()> {
    let labels: Vec<&str> = ui::AGENT_TYPES.iter().map(|(_, label)| *label).collect();
    let choice = ui::select_option("What kind of agent is this?", &labels)?;
    let (agent_name, label) = ui::AGENT_TYPES[choice];

    let display = ui::input(&format!("Bot display name (e.g. {label} Bot)"))?;
    agent_create(ctx.clone(), agent_name, &display).await
}

pub async fn workspace_init(ctx: Context, path: &str) -> Result<()> {
    if ctx.menu_mode && !ctx.suppress_screen {
        action_screen(&ctx)?;
    }

    if config::workspace_exists(Path::new(path))? {
        return Err(anyhow!(
            "this folder already has a workspace — use Project → Manage workspaces to update or remove it"
        ));
    }

    let session = keychain::load_session()?;
    let orgs = ctx
        .api()
        .get::<OrgListResponse>("/orgs", Some(&session))
        .await?;
    let org = resolve_org_for_workspace_init(&ctx, &orgs).await?;

    let workspace = config::init_workspace(Path::new(path), org.clone())?;
    if let Some(remote_owner) = github::remote_owner_at(&workspace.workspace_path) {
        ui::warn_org_mismatch(
            &org,
            &remote_owner,
            "This workspace will request GitHub tokens for the AgentID org above.",
        );
    }
    let mut next_global = config::load_global().unwrap_or_default();
    next_global.workspace_path = Some(workspace.workspace_path.clone());
    next_global.selected_org = Some(org);
    config::save_global(&next_global)?;
    ui::success(&format!(
        "Workspace ready at {}",
        workspace.workspace_path.display()
    ));
    Ok(())
}

pub fn workspace_scan(ctx: &Context) -> Result<()> {
    if ctx.menu_mode && !ctx.suppress_screen {
        let session = keychain::load_session()?;
        ui::begin_action_screen(&session.user_email);
    }
    let mut workspace = config::load_workspace()?;
    let repos = github::scan_repositories(&workspace.workspace_path)?;
    workspace.detected_repos = repos.clone();
    config::save_workspace(&workspace)?;
    if repos.is_empty() {
        ui::info("No git repositories found.");
    } else if ctx.menu_mode {
        ui::print_table(
            &["REPOSITORY"],
            &repos
                .iter()
                .map(|repo| vec![repo.clone()])
                .collect::<Vec<_>>(),
        );
    } else {
        ui::heading("Repositories");
        for repo in repos {
            println!("  • {repo}");
        }
    }
    Ok(())
}

pub async fn project_menu(ctx: Context) -> Result<()> {
    loop {
        action_screen(&ctx)?;
        maybe_heading(&ctx, "Project");
        print_project_context(&ctx).await?;

        let choice = ui::select_option(
            "This folder",
            &[
                "Initialize this folder",
                "Scan repositories",
                "Grant repo access to a bot",
                "Activate a bot for git",
                "Manage workspaces",
                "Back",
            ],
        )?;
        match choice {
            0 => workspace_init(ctx.clone(), ".").await,
            1 => workspace_scan(&ctx),
            2 => allow_interactive(ctx.clone()).await,
            3 => use_agent(ctx.clone(), None).await,
            4 => workspace_menu(ctx.clone()).await,
            5 => return Ok(()),
            _ => return Ok(()),
        }?;
    }
}

enum ProjectAction {
    Init,
    Scan,
    Allow,
    Use,
}

async fn project_action(ctx: Context, action: ProjectAction) -> Result<()> {
    action_screen(&ctx)?;
    maybe_heading(&ctx, "Project");
    print_project_context(&ctx).await?;
    match action {
        ProjectAction::Init => workspace_init(ctx, ".").await,
        ProjectAction::Scan => workspace_scan(&ctx),
        ProjectAction::Allow => allow_interactive(ctx).await,
        ProjectAction::Use => use_agent(ctx, None).await,
    }
}

async fn resolve_org_for_workspace_init(ctx: &Context, orgs: &OrgListResponse) -> Result<String> {
    let global = config::load_global().unwrap_or_default();
    if let Some(selected) = global.selected_org.as_deref() {
        if org_is_connected(selected, Some(orgs)) {
            org_select(ctx.clone(), selected).await?;
            return Ok(selected.to_string());
        }
    }
    if let Some(remote) = github::current_remote_owner() {
        if org_is_connected(&remote, Some(orgs)) {
            org_select(ctx.clone(), &remote).await?;
            return Ok(remote);
        }
    }
    if orgs.orgs.is_empty() {
        return Err(anyhow!(
            "no organizations connected — use GitHub → Organizations to connect one first"
        ));
    }
    pick_connected_org(ctx, "Which organization is this workspace for?").await
}

async fn require_connected_workspace_org(ctx: &Context) -> Result<config::WorkspaceConfig> {
    let workspace = config::load_workspace()?;
    let session = keychain::load_session()?;
    let orgs = ctx
        .api()
        .get::<OrgListResponse>("/orgs", Some(&session))
        .await?;
    if !org_is_connected(&workspace.org, Some(&orgs)) {
        return Err(anyhow!(
            "organization `{}` is disconnected from AgentID — reconnect it under GitHub → Organizations, or change the workspace org under Manage workspaces",
            workspace.org
        ));
    }
    let global = config::load_global().unwrap_or_default();
    if global.selected_org.as_deref() != Some(workspace.org.as_str()) {
        org_select(ctx.clone(), &workspace.org).await?;
    }
    Ok(workspace)
}

async fn print_project_context(ctx: &Context) -> Result<()> {
    let session = keychain::load_session()?;
    let orgs = ctx
        .api()
        .get::<OrgListResponse>("/orgs", Some(&session))
        .await
        .ok();

    if let Ok(workspace) = config::load_workspace() {
        let mut lines = vec![
            ("Folder", workspace.workspace_path.display().to_string()),
            (
                "Organization",
                org_connection_label(&workspace.org, orgs.as_ref()),
            ),
            (
                "Active bot",
                workspace
                    .active_agent
                    .clone()
                    .unwrap_or_else(|| "none".to_string()),
            ),
            ("Repos scanned", workspace.detected_repos.len().to_string()),
        ];
        if let Some(remote_owner) = github::current_remote_owner() {
            if !github::org_matches_remote(&workspace.org, &remote_owner) {
                lines.push((
                    "Git remote",
                    format!("⚠ owner `{remote_owner}` ≠ org `{}`", workspace.org),
                ));
            } else {
                lines.push(("Git remote", remote_owner));
            }
        }
        ui::status_panel(&lines);
        if !org_is_connected(&workspace.org, orgs.as_ref()) {
            ui::info(
                "This workspace's organization is disconnected from AgentID.\n\
                 Reconnect via GitHub → Organizations, or change the workspace org here.",
            );
        }
        ui::blank();
    } else {
        ui::info("This folder is not initialized yet — run Initialize this folder first.");
        ui::blank();
    }
    Ok(())
}

pub async fn workspace_menu(ctx: Context) -> Result<()> {
    loop {
        action_screen(&ctx)?;
        maybe_heading(&ctx, "Workspaces");

        let workspaces = config::list_known_workspaces()?;
        if workspaces.is_empty() {
            ui::info("No workspaces registered yet.");
            if ui::confirm_step("Initialize a workspace in this folder?")? {
                workspace_init(ctx.clone(), ".").await?;
            }
            return Ok(());
        }

        let rows: Vec<Vec<String>> = workspaces
            .iter()
            .map(|workspace| {
                let active = workspace
                    .active_agent
                    .clone()
                    .unwrap_or_else(|| "—".to_string());
                let repos = workspace.detected_repos.len().to_string();
                vec![
                    workspace.org.clone(),
                    workspace.workspace_path.display().to_string(),
                    active,
                    repos,
                ]
            })
            .collect();
        ui::print_table(&["ORG", "PATH", "ACTIVE BOT", "REPOS"], &rows);
        ui::blank();

        let mut options: Vec<String> = workspaces
            .iter()
            .map(|workspace| format!("{} — {}", workspace.org, workspace.workspace_path.display()))
            .collect();
        options.push("Back".to_string());
        let labels: Vec<&str> = options.iter().map(String::as_str).collect();
        let choice = ui::select_option("Manage which workspace?", &labels)?;
        if choice == workspaces.len() {
            return Ok(());
        }
        let workspace = workspaces[choice].clone();

        loop {
            let action = ui::select_option(
                &format!("Workspace: {}", workspace.workspace_path.display()),
                &[
                    "View details",
                    "Rescan repositories",
                    "Clear active bot",
                    "Change organization",
                    "Remove workspace",
                    "Back",
                ],
            )?;

            match action {
                0 => show_workspace_details(&workspace),
                1 => workspace_rescan_at(&workspace)?,
                2 => workspace_clear_active_agent(&workspace)?,
                3 => workspace_change_org(&ctx, &workspace).await?,
                4 => {
                    workspace_remove(&workspace)?;
                    return Ok(());
                }
                _ => break,
            }
        }
    }
}

fn show_workspace_details(workspace: &config::WorkspaceConfig) {
    ui::status_panel(&[
        ("Organization", workspace.org.clone()),
        ("Path", workspace.workspace_path.display().to_string()),
        (
            "Active bot",
            workspace
                .active_agent
                .clone()
                .unwrap_or_else(|| "none".to_string()),
        ),
        (
            "Repositories",
            if workspace.detected_repos.is_empty() {
                "none scanned".to_string()
            } else {
                workspace.detected_repos.join(", ")
            },
        ),
    ]);
}

fn workspace_rescan_at(workspace: &config::WorkspaceConfig) -> Result<()> {
    let mut updated = workspace.clone();
    let repos = github::scan_repositories(&updated.workspace_path)?;
    updated.detected_repos = repos.clone();
    config::save_workspace(&updated)?;
    if repos.is_empty() {
        ui::info("No git repositories found.");
    } else {
        ui::print_table(
            &["REPOSITORY"],
            &repos
                .iter()
                .map(|repo| vec![repo.clone()])
                .collect::<Vec<_>>(),
        );
        ui::success(&format!("Found {} repositories.", repos.len()));
    }
    Ok(())
}

fn workspace_clear_active_agent(workspace: &config::WorkspaceConfig) -> Result<()> {
    let mut updated = workspace.clone();
    if updated.active_agent.is_none() {
        ui::info("No active bot in this workspace.");
        return Ok(());
    }
    let agent = updated.active_agent.take().unwrap();
    config::save_workspace(&updated)?;
    ui::success(&format!("Cleared active bot `{agent}`."));
    Ok(())
}

async fn workspace_change_org(ctx: &Context, workspace: &config::WorkspaceConfig) -> Result<()> {
    let org = pick_connected_org(ctx, "Assign which organization to this workspace?").await?;
    if let Some(remote_owner) = github::remote_owner_at(&workspace.workspace_path) {
        ui::warn_org_mismatch(
            &org,
            &remote_owner,
            "Commits and push in this workspace should use the same org as `git remote`.",
        );
    }
    if !ui::confirm_step(&format!(
        "Change workspace org from `{}` to `{org}`?",
        workspace.org
    ))? {
        ui::info("Cancelled.");
        return Ok(());
    }
    let mut updated = workspace.clone();
    updated.org = org.clone();
    config::save_workspace(&updated)?;
    ui::success(&format!("Workspace org is now `{org}`."));
    Ok(())
}

fn workspace_remove(workspace: &config::WorkspaceConfig) -> Result<()> {
    ui::info(&format!(
        "Removes `.agentid/workspace.yaml` at {}.\n\
         Does not delete your git repos or bots on the server.",
        workspace.workspace_path.display()
    ));
    if !ui::confirm_step(&format!(
        "Remove workspace at {}?",
        workspace.workspace_path.display()
    ))? {
        ui::info("Cancelled.");
        return Ok(());
    }
    config::delete_workspace(&workspace.workspace_path)?;
    ui::success(&format!(
        "Removed workspace at {}.",
        workspace.workspace_path.display()
    ));
    Ok(())
}

pub async fn allow(
    ctx: Context,
    agent: &str,
    repos: Option<&str>,
    all_detected: bool,
    branches: &str,
) -> Result<()> {
    let workspace = config::load_workspace()?;
    let selected_repos = if all_detected {
        workspace.detected_repos.clone()
    } else {
        split_csv(repos.ok_or_else(|| anyhow!("use --repos or --all"))?)
    };
    if selected_repos.is_empty() {
        return Err(anyhow!("no repositories selected"));
    }
    if let Some(remote_owner) = github::current_remote_owner() {
        ui::warn_org_mismatch(
            &workspace.org,
            &remote_owner,
            "Permissions are stored for the workspace org; push auth also uses it.",
        );
        if let Ok(global) = config::load_global() {
            if let Some(selected_org) = global.selected_org.as_deref() {
                if !github::org_matches_remote(selected_org, &workspace.org) {
                    ui::warn_org_mismatch(
                        selected_org,
                        &remote_owner,
                        "The globally selected org differs from this workspace and is used for `allow`/`agents`.",
                    );
                }
            }
        }
    }
    let selected_branches = split_csv(branches);
    let session = keychain::load_session()?;
    let _: serde_json::Value = ctx
        .api()
        .post(
            "/permissions",
            &json!({ "agent": agent, "repos": selected_repos, "branches": selected_branches }),
            Some(&session),
        )
        .await?;
    ui::success(&format!(
        "{} can push to: {}",
        agent,
        selected_repos.join(", ")
    ));
    Ok(())
}

pub async fn use_agent(ctx: Context, agent_name: Option<&str>) -> Result<()> {
    if !ctx.menu_mode {
        action_screen(&ctx)?;
    }
    let workspace = require_connected_workspace_org(&ctx).await?;
    let agent_name = match agent_name {
        Some(name) => name.to_string(),
        None => pick_agent(&ctx, &workspace.org).await?,
    };
    let session = keychain::load_session()?;
    let api = ctx.api();
    let agents: AgentListResponse = api
        .get(
            &format!("/agents?org={}", url_encode(&workspace.org)),
            Some(&session),
        )
        .await?;
    let agent = agents
        .agents
        .into_iter()
        .find(|agent| agent.name == agent_name)
        .ok_or_else(|| anyhow!("agent `{agent_name}` not found"))?;
    let permissions: PermissionListResponse = api.get("/permissions", Some(&session)).await?;
    let permission = permissions
        .permissions
        .into_iter()
        .find(|permission| permission.agent == agent_name)
        .ok_or_else(|| anyhow!("agent `{agent_name}` has no permissions — run `agentid allow`"))?;

    let mut workspace = config::load_workspace()?;
    workspace.active_agent = Some(agent_name.clone());
    config::save_workspace(&workspace)?;

    if let Some(remote_owner) = github::current_remote_owner() {
        ui::warn_org_mismatch(
            &workspace.org,
            &remote_owner,
            "Git push will request tokens for the AgentID org above, not the remote owner.",
        );
    }

    let mut configured = std::collections::BTreeSet::new();
    for repo in &permission.repos {
        if let Some(path) = github::repo_path(&workspace.workspace_path, repo) {
            github::configure_repo(&path, &agent.display_name, BOT_COMMIT_EMAIL)?;
            configured.insert(path);
        }
    }

    if let Ok((repo, _)) = github::current_repo_and_branch() {
        if permission.repos.contains(&repo) {
            if let Ok(root) = github::repo_root() {
                if configured.insert(root.clone()) {
                    github::configure_repo(&root, &agent.display_name, BOT_COMMIT_EMAIL)?;
                }
            }
        }
    }

    ui::success(&format!(
        "Active bot: {} ({})",
        agent.display_name, agent.name
    ));
    ui::info("Commits and pushes in this folder will use this identity.");
    Ok(())
}

pub async fn status(ctx: Context) -> Result<()> {
    action_screen(&ctx)?;
    maybe_heading(&ctx, "Status");
    let session = keychain::load_session()?;
    let global = config::load_global().unwrap_or_default();
    let workspace = config::load_workspace().ok();
    let orgs = ctx
        .api()
        .get::<OrgListResponse>("/orgs", Some(&session))
        .await
        .ok();

    let mut lines = vec![("Account", session.user_email.clone())];

    if let Some(workspace) = &workspace {
        let org_label = org_connection_label(&workspace.org, orgs.as_ref());
        lines.push(("Organization", org_label));
        lines.push(("Workspace", workspace.workspace_path.display().to_string()));
        if let Some(agent) = workspace.active_agent.as_deref() {
            lines.push(("Active bot", agent.to_string()));
        }
        if let Some(remote_owner) = github::current_remote_owner() {
            if !github::org_matches_remote(&workspace.org, &remote_owner) {
                lines.push((
                    "Git remote",
                    format!("⚠ owner `{remote_owner}` ≠ org `{}`", workspace.org),
                ));
            } else {
                lines.push(("Git remote", remote_owner));
            }
        }
        if !org_is_connected(&workspace.org, orgs.as_ref()) {
            ui::status_panel(&lines);
            ui::info(
                "This workspace's organization is disconnected from AgentID.\n\
                 Reconnect via GitHub → Organizations, or change the workspace org in Project workflow.",
            );
            print_permissions(&ctx, &session).await?;
            return Ok(());
        }
    } else if let Some(selected) = orgs
        .as_ref()
        .and_then(|response| response.selected_org.as_deref())
        .or(global.selected_org.as_deref())
    {
        lines.push((
            "Organization",
            org_connection_label(selected, orgs.as_ref()),
        ));
    }

    ui::status_panel(&lines);
    print_permissions(&ctx, &session).await?;
    Ok(())
}

async fn print_permissions(ctx: &Context, session: &Session) -> Result<()> {
    if let Ok(permissions) = ctx
        .api()
        .get::<PermissionListResponse>("/permissions", Some(session))
        .await
    {
        if permissions.permissions.is_empty() {
            ui::info("No repository permissions yet — run `agentid allow`.");
        } else if ctx.menu_mode {
            let rows: Vec<Vec<String>> = permissions
                .permissions
                .iter()
                .map(|permission| {
                    vec![
                        permission.agent.clone(),
                        permission.repos.join(", "),
                        permission.branches.join(", "),
                    ]
                })
                .collect();
            ui::print_table(&["AGENT", "REPOS", "BRANCHES"], &rows);
        } else {
            ui::heading("Permissions");
            for permission in permissions.permissions {
                println!(
                    "  • {} → {} [{}]",
                    permission.agent,
                    permission.repos.join(", "),
                    permission.branches.join(", ")
                );
            }
        }
    }
    Ok(())
}

fn org_is_connected(org: &str, orgs: Option<&OrgListResponse>) -> bool {
    orgs.is_some_and(|response| response.orgs.iter().any(|item| item.org == org))
}

fn org_connection_label(org: &str, orgs: Option<&OrgListResponse>) -> String {
    let Some(orgs) = orgs else {
        return org.to_string();
    };
    if !org_is_connected(org, Some(orgs)) {
        return format!("{org} (disconnected)");
    }
    if orgs.selected_org.as_deref() == Some(org) {
        return format!("{org} (active)");
    }
    format!("{org} (connected)")
}

pub async fn device_revoke(ctx: Context) -> Result<()> {
    let session = keychain::load_session()?;
    let _: serde_json::Value = ctx
        .api()
        .post("/devices/revoke", &json!({}), Some(&session))
        .await?;
    keychain::delete_session()?;
    ui::success("Device revoked and signed out.");
    Ok(())
}

pub async fn git_credential(ctx: Context, operation: Option<&str>) -> Result<()> {
    if !matches!(operation.unwrap_or("get"), "get") {
        return Ok(());
    }

    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let values = parse_git_credential_input(&input);
    if values.get("host").map(String::as_str) != Some("github.com") {
        return Ok(());
    }

    let session = keychain::load_session()?;
    let workspace = config::load_workspace()?;
    let agent = workspace
        .active_agent
        .ok_or_else(|| anyhow!("run `agentid use <agent>` first"))?;
    let (remote_owner, repo, branch) = github::current_repo_owner_and_branch()?;
    if !github::org_matches_remote(&workspace.org, &remote_owner) {
        return Err(anyhow!(
            "AgentID org '{}' does not match git remote owner '{}'. Run `agentid orgs` or update `git remote set-url origin`.",
            workspace.org,
            remote_owner
        ));
    }
    let response: GitHubTokenResponse = ctx
        .api()
        .post(
            "/github/token",
            &json!({
                "org": workspace.org,
                "repo": repo,
                "branch": branch,
                "agent": agent,
                "operation": "push"
            }),
            Some(&session),
        )
        .await?;

    println!("username={}", response.username);
    println!("password={}", response.token);
    println!();
    Ok(())
}

fn split_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|part| part.trim().to_lowercase())
        .filter(|part| !part.is_empty())
        .collect()
}

async fn pick_connected_org(ctx: &Context, prompt: &str) -> Result<String> {
    let session = keychain::load_session()?;
    let orgs: OrgListResponse = ctx.api().get("/orgs", Some(&session)).await?;
    if orgs.orgs.is_empty() {
        return Err(anyhow!(
            "no organizations connected — run `agentid github` first"
        ));
    }

    let rows: Vec<Vec<String>> = orgs
        .orgs
        .iter()
        .map(|org| {
            let marker = if orgs.selected_org.as_deref() == Some(org.org.as_str()) {
                " ← active"
            } else {
                ""
            };
            vec![format!("{}{}", org.org, marker), org.account_login.clone()]
        })
        .collect();
    ui::print_table(&["ORG", "ACCOUNT"], &rows);
    ui::blank();

    let options: Vec<String> = orgs
        .orgs
        .iter()
        .map(|org| {
            if orgs.selected_org.as_deref() == Some(org.org.as_str()) {
                format!("{} (active)", org.org)
            } else {
                org.org.clone()
            }
        })
        .collect();
    let labels: Vec<&str> = options.iter().map(String::as_str).collect();
    let choice = ui::select_option(prompt, &labels)?;
    Ok(orgs.orgs[choice].org.clone())
}

fn url_encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn required(value: Option<String>, field: &str) -> Result<String> {
    value.ok_or_else(|| anyhow!("API response missing `{}`", field))
}

fn parse_git_credential_input(input: &str) -> HashMap<String, String> {
    input
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

#[allow(dead_code)]
fn _keep_global_config_used(_: GlobalConfig) {}
