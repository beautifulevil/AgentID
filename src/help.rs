use console::style;

pub fn print_help() {
    println!();
    println!("  {}", style("AgentID").bold().cyan());
    println!(
        "  {}",
        style("Git identity for AI coding agents (Cursor, Codex, Claude Code, …)").dim()
    );
    println!();

    section(
        "What it is",
        &[
            "AgentID gives your AI agents a controlled GitHub identity.",
            "You do not store PATs, .pem keys, or long-lived GitHub tokens in the CLI.",
            "Commits use a bot display name; git push gets short-lived tokens from AgentID.",
        ],
    );

    section(
        "How it works",
        &[
            "1. Sign in with your email (one-time code, no password).",
            "2. Install the AgentID GitHub App on your organization.",
            "3. Register bots (git identities) for each agent.",
            "4. Initialize a workspace in your project folder (`agentid init`).",
            "5. Grant each bot access to repos and branches (`agentid allow`).",
            "6. Activate a bot locally (`agentid use`) — commits and push use that identity.",
        ],
    );

    section(
        "Important rules",
        &[
            "The GitHub org in AgentID must match the owner in `git remote`.",
            "  Example: remote `dodeys/my-repo` → org must be `dodeys`.",
            "One `agentid init` per project folder (`.agentid/workspace.yaml`).",
            "Global org selection affects `agents` / `allow`; push uses the workspace org.",
            "Requires GitHub CLI (`gh`) for org install flows.",
        ],
    );

    section(
        "Menu structure",
        &[
            "Setup — wizard and status overview",
            "GitHub — organizations (connect / active / disconnect) and bots",
            "Project — init, scan, allow, use, manage workspaces (this folder)",
            "Account — help, delete account, sign out",
        ],
    );

    section(
        "Quick start",
        &[
            "agentid              Interactive menu (login if needed)",
            "agentid login        Sign in or create account",
            "agentid github       Organizations menu",
            "agentid init .       Create workspace in this folder",
            "agentid workspace    Project workflow menu",
            "agentid scan         Find git repos in the workspace",
            "agentid allow        Grant repo access to a bot",
            "agentid use          Activate a bot for git in this folder",
            "git commit && git push",
        ],
    );

    section(
        "Commands",
        &[
            "agentid              Main menu",
            "agentid login        Email sign-in / sign-up",
            "agentid login status Session info",
            "agentid status       Account, org, workspace, permissions",
            "agentid orgs         Organizations menu",
            "agentid org <name>   Select active organization",
            "agentid github       Organizations menu",
            "agentid agents       Bots menu (register / delete)",
            "agentid new          Register a new bot",
            "agentid init <path>  Create `.agentid/workspace.yaml`",
            "agentid workspace    Project workflow menu",
            "agentid scan         Detect git repositories",
            "agentid allow        Grant push access (interactive or flags)",
            "agentid use          Set active bot for local git",
            "agentid settings     Account & security (delete account, sign out)",
            "agentid revoke       Sign out this device",
            "agentid logout       Same as revoke",
            "agentid help         Show this help",
        ],
    );

    section(
        "Install",
        &[
            "Homebrew: brew tap beautifulevil/tap && brew install agentid",
            "From source: cargo install --path agentid-cli",
        ],
    );

    section(
        "Files",
        &[
            "~/.config/agentid/session.json   Your login session",
            "~/.config/agentid/config.json    Selected org and known workspace paths",
            ".agentid/workspace.yaml          Org, active bot, scanned repos (per project)",
        ],
    );

    println!(
        "  {}",
        style("More: project.md and agentid-cli/README.md in the repository.").dim()
    );
    println!();
}

fn section(title: &str, lines: &[&str]) {
    println!("  {}", style(title).bold());
    for line in lines {
        if line.starts_with("  ") {
            println!("  {}", style(line).dim());
        } else {
            println!("    {line}");
        }
    }
    println!();
}
