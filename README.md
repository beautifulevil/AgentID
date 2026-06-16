# AgentID

AgentID gives AI coding agents a controlled GitHub identity. Developers do not handle GitHub PATs, private keys, or long-lived tokens in the CLI.

## Install

```bash
brew tap beautifulevil/tap
brew install agentid
```

Or build from source:

```bash
cargo install --path .
```

## First Run

```bash
agentid
```

If you are not signed in yet, AgentID starts the email-code login flow in the terminal. New accounts are created automatically after email verification.

## Typical Workflow

```bash
agentid github      # connect a GitHub organization
agentid orgs        # choose the active organization
agentid agents      # register or manage bot identities
agentid init .      # initialize the current project folder
agentid scan        # detect Git repositories in the project
agentid allow       # choose bot, repositories, and branches
agentid use         # activate a bot identity for this folder
```

After setup, normal `git push` uses AgentID as a Git credential helper and requests short-lived GitHub App installation tokens from the AgentID API.

## Useful Commands

```bash
agentid                 # interactive menu
agentid login           # sign in or create account
agentid login status    # check local session
agentid status          # account, organization, and workspace summary
agentid settings        # account cleanup and sign out
agentid revoke          # sign out this device
```

Session data is stored at `~/.config/agentid/session.json` with `0600` permissions.

## Requirements

- GitHub CLI (`gh`) for organization discovery and GitHub App installation flow.
- HTTPS Git remotes are recommended.

## License

MIT
