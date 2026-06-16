use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use regex::Regex;
use walkdir::WalkDir;

pub fn scan_repositories(workspace_path: &Path) -> Result<Vec<String>> {
    let mut repos = BTreeSet::new();
    for entry in WalkDir::new(workspace_path)
        .max_depth(4)
        .into_iter()
        .filter_entry(|entry| !is_ignored(entry.path()))
    {
        let entry = entry?;
        if !entry.file_type().is_dir() {
            continue;
        }
        if entry.path().join(".git").exists() {
            if let Ok(remote) = git_output(entry.path(), ["remote", "get-url", "origin"]) {
                if let Some((_owner, repo)) = parse_github_remote(remote.trim()) {
                    repos.insert(repo);
                }
            }
        }
    }
    Ok(repos.into_iter().collect())
}

pub fn repo_root() -> Result<PathBuf> {
    let root = git_output(Path::new("."), ["rev-parse", "--show-toplevel"])?;
    Ok(PathBuf::from(root.trim().to_string()))
}

pub fn current_repo_owner_and_branch() -> Result<(String, String, String)> {
    let root = repo_root()?;
    let branch = git_output(&root, ["branch", "--show-current"])?
        .trim()
        .to_string();
    if branch.is_empty() {
        return Err(anyhow!("could not determine current Git branch"));
    }
    let remote = git_output(&root, ["remote", "get-url", "origin"])?;
    let (owner, repo) = parse_github_remote(remote.trim())
        .ok_or_else(|| anyhow!("origin is not a GitHub remote"))?;
    Ok((owner, repo, branch))
}

pub fn current_repo_and_branch() -> Result<(String, String)> {
    let (_owner, repo, branch) = current_repo_owner_and_branch()?;
    Ok((repo, branch))
}

pub fn remote_owner_at(path: &Path) -> Option<String> {
    let remote = git_output(path, ["remote", "get-url", "origin"]).ok()?;
    parse_github_remote(remote.trim()).map(|(owner, _)| owner)
}

pub fn current_remote_owner() -> Option<String> {
    repo_root().ok().and_then(|root| remote_owner_at(&root))
}

pub fn org_matches_remote(agentid_org: &str, remote_owner: &str) -> bool {
    agentid_org.eq_ignore_ascii_case(remote_owner)
}

pub fn configure_repo(path: &Path, display: &str, email: &str) -> Result<()> {
    run_git(path, ["config", "user.name", display])?;
    run_git(path, ["config", "user.email", email])?;
    let helper = format!("!{} git-credential", shell_quote(&std::env::current_exe()?));
    run_git(path, ["config", "credential.helper", helper.as_str()])?;
    Ok(())
}

pub fn list_user_organizations() -> Result<Vec<String>> {
    let output = Command::new("gh")
        .args(["api", "user/orgs", "--jq", ".[].login"])
        .output()
        .context("failed to run `gh`; install GitHub CLI and run `gh auth login`")?;
    if !output.status.success() {
        return Err(anyhow!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let orgs: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_lowercase)
        .collect();
    if orgs.is_empty() {
        return Err(anyhow!(
            "no GitHub organizations found; make sure `gh auth login` is complete"
        ));
    }
    Ok(orgs)
}

pub fn repo_path(workspace_path: &Path, repo: &str) -> Option<PathBuf> {
    for entry in WalkDir::new(workspace_path)
        .max_depth(4)
        .into_iter()
        .filter_entry(|entry| !is_ignored(entry.path()))
        .flatten()
    {
        if entry.file_type().is_dir() && entry.path().join(".git").exists() {
            if let Ok(remote) = git_output(entry.path(), ["remote", "get-url", "origin"]) {
                if let Some((_owner, name)) = parse_github_remote(remote.trim()) {
                    if name == repo {
                        return Some(entry.path().to_path_buf());
                    }
                }
            }
        }
    }
    None
}

pub fn parse_github_remote(remote: &str) -> Option<(String, String)> {
    let patterns = [
        r"^git@github\.com:(?P<owner>[^/]+)/(?P<repo>[^/]+?)(?:\.git)?$",
        r"^https://github\.com/(?P<owner>[^/]+)/(?P<repo>[^/]+?)(?:\.git)?$",
    ];
    for pattern in patterns {
        let regex = Regex::new(pattern).ok()?;
        if let Some(captures) = regex.captures(remote) {
            return Some((
                captures["owner"].to_lowercase(),
                captures["repo"].to_lowercase(),
            ));
        }
    }
    None
}

fn git_output<const N: usize>(path: &Path, args: [&str; N]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .with_context(|| format!("failed to run git in {}", path.display()))?;
    if !output.status.success() {
        return Err(anyhow!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn run_git<const N: usize>(path: &Path, args: [&str; N]) -> Result<()> {
    let output = Command::new("git").args(args).current_dir(path).output()?;
    if !output.status.success() {
        return Err(anyhow!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

fn is_ignored(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    matches!(name, "node_modules" | "target" | ".wrangler" | ".git")
}

fn shell_quote(path: &Path) -> String {
    let value = path.to_string_lossy();
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{org_matches_remote, parse_github_remote, shell_quote};

    #[test]
    fn org_match_is_case_insensitive() {
        assert!(org_matches_remote("dodeys", "Dodeys"));
        assert!(!org_matches_remote("dodeys", "beautifulevil"));
    }

    #[test]
    fn parses_https_remote() {
        assert_eq!(
            parse_github_remote("https://github.com/beautifulevil/agentid-api.git"),
            Some(("beautifulevil".to_string(), "agentid-api".to_string()))
        );
    }

    #[test]
    fn parses_ssh_remote() {
        assert_eq!(
            parse_github_remote("git@github.com:beautifulevil/agentid-cli.git"),
            Some(("beautifulevil".to_string(), "agentid-cli".to_string()))
        );
    }

    #[test]
    fn shell_quotes_helper_paths() {
        assert_eq!(
            shell_quote(Path::new("/tmp/Mobile Documents/agentid")),
            "'/tmp/Mobile Documents/agentid'"
        );
        assert_eq!(
            shell_quote(Path::new("/tmp/agent'id")),
            "'/tmp/agent'\\''id'"
        );
    }
}
