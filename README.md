# AgentID

AgentID is a CLI that gives AI coding agents (Cursor, Codex, Claude Code, etc.) a **scoped GitHub identity** under your organization's control — without handing them a PAT, SSH key, or GitHub App private key.

You register **bot identities** per org, explicitly allow which repositories and branches each bot can push to, then activate one per project folder. When the agent runs `git push`, AgentID acts as a Git credential helper: it authenticates you with a device session, checks the bot's policy server-side, and injects a **short-lived GitHub App installation token** scoped to that single repo. The token lives for one push and is never written to disk.

Developers manage access through the CLI (`agentid allow`, `agentid use`). Admins install one GitHub App on the org; the App's `.pem` stays on the server. Audit logs record who requested tokens, for which repo/branch, and whether access was granted or denied.

The same model applies to **CI/CD**: each bot gets its own Git identity (`user.name` / `user.email` via `agentid use`), so commits and pushes in a pipeline are attributable to a named agent — not a shared org token or a human's credentials. In GitHub you can tell which bot pushed a branch; in AgentID's audit log you can correlate token requests with bot, device, org, and branch. That makes it easier to scope automation per pipeline, enforce branch policy per agent, and trace what an autonomous job actually did.

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

## Security & Credential Model

Three tiers: server-held crypto, CLI device session, JIT GitHub tokens. No PATs, SSH keys, or App private keys on the client.

### CLI persistence

The `keychain` module writes JSON — not macOS Keychain / Secret Service:

| File | Path | Contents |
|------|------|----------|
| Session | `~/.config/agentid/session.json` | Cleartext bearer + refresh tokens, device ID, email, expiry (`0600` on Unix) |
| Config | `~/.config/agentid/config.json` | Org selection, workspace paths |
| Workspace | `<project>/.agentid/workspace.yaml` | Active bot, org, detected repos |

The CLI holds session tokens in cleartext locally — same tradeoff as `~/.git-credentials` or a cached OAuth token. `0600` limits exposure to other OS users; remote revocation is via `agentid revoke`. In-process cache is flushed on save/delete.

GitHub installation tokens are never persisted. The credential helper requests one per `git push`, writes `username`/`password` to Git's stdout pipe, exits.

### Application cryptography

All server-side digests use **peppered SHA-256**: `hex(SHA-256(pepper + ":" + value))`. Comparison via constant-time equality. Random material from `crypto.getRandomValues` (32-byte tokens, base64url-encoded with prefix).

| Material | Stored on server | On wire to client |
|----------|----------------|-----------------|
| Session / refresh token | Hash only (Postgres + Redis index) | Cleartext once at login, then Bearer header |
| Email / pairing codes | Hash only | Cleartext once via email or internal CLI channel |
| GitHub App private key | Secret Manager (KMS-wrapped) | Never |
| GitHub installation token | Not stored | Once per push, ~1h GitHub TTL |

**GitHub App JWT** — RS256, `iat`/`exp` window ~9 min, signed in-process after KMS decrypt of the PEM key. **Webhooks** — HMAC-SHA256 over raw body, `sha256=` prefix, constant-time verify.

### Backend layout

```
                         ┌───────────────┐
                         │ Load Balancer │
                         └───────┬───────┘
                                 │
     ┌─────────────┐      ┌───────▼───────┐      ┌─────────────┐
     │     CLI     │─────►│   Cloud Run   │◄────►│   GitHub    │
     │   agentid   │      │  agentid-api  │      │  App API    │
     └─────────────┘      └───────┬───────┘      └─────────────┘
                                  │
            ┌─────────────────────┼─────────────────────┐
            │                     │                     │
     ┌──────▼──────┐      ┌───────▼───────┐     ┌──────▼──────┐
     │  Cloud SQL  │      │Secret Manager │     │ Memorystore │
     │  (Postgres) │      │   + KMS       │     │   (Redis)   │
     └─────────────┘      └───────────────┘     └─────────────┘
```

| Service | Configuration |
|---------|---------------|
| **Cloud Run** | `us-central1`, 1 vCPU / 512 MiB, concurrency 80, min 1 / max 20 instances, ingress internal-and-cloud-load-balancing |
| **Cloud SQL** | Postgres 15, `db-custom-2-4096`, regional HA, private IP via VPC peering, automated backups + PITR (7 days) |
| **Memorystore** | Redis 7.0, 1 GiB, `AUTH` enabled, `allkeys-lru`, transit encryption on |
| **VPC** | `agentid-prod-vpc`, `/20` subnet, Private Service Connect to SQL + Redis, Cloud NAT for GitHub/email egress |
| **Load balancer** | Managed TLS cert, HTTPS redirect, backend timeout 30s |
| **Cloud Armor** | Rate-based ban (100 req/min/IP), OWASP CRS preview ruleset |
| **KMS** | `global` keyring — symmetric CMK for Secret Manager envelope; separate asymmetric key for optional field-level encrypt |
| **Secret Manager** | Versioned secrets, IAM `secretAccessor` scoped per-secret to Cloud Run SA |
| **Logging** | Structured JSON → Cloud Logging; audit table mirrored for query API; 90-day retention |

Postgres holds authoritative state: users, devices, sessions (hashed), agents, permissions, GitHub installations, audit log. Redis holds hot paths — session lookup by digest, login/pairing state, rate-limit counters — all keys with explicit TTL.

### Session resolution

On each authenticated request:

1. Extract Bearer token → compute digest → Redis `GET session:{digest}`
2. Check `expires_at` on the cached record
3. Postgres join `sessions` ↔ `cli_devices` — reject if session revoked, device revoked, or `status != active`

Revocation is authoritative in Postgres; Redis may lag until TTL. Device revoke sets `cli_devices.status = revoked` and stamps all active sessions — subsequent requests fail at step 3 even with a warm cache.

TTL: session 30d, refresh 90d. Both digests written to Postgres; Redis keys inherit matching `EXPIRE`.

### Auth flow (sign-in / registration)

Sign-in and account creation share one path — `upsertUser` on verify creates the account if the email is new. No password.

```
        CLI                   API                   Store
        |                     |                     |
  1     | device/start   -->  | hash(codes)    -->  | Postgres
        | <-- codes           |                     |
        |                     |                     |
  2     | email/start    -->  | hash(otp)      -->  | Postgres --> Email
        | <-- request_id      |                     |
        |                     |                     |
  3     | email/verify   -->  | upsert user    -->  | Postgres
        |                     | hash(session)  -->  | PG + Redis
        | <-- ok              |                     |
        |                     |                     |
  4     | device/poll    -->  | read staging   <--  | Redis
        | <-- tokens          |                     |
        |                     |                     |
  5     | session.json        | complete       -->  | Redis DEL
```

**1 — Register device.** API generates `pairing_code` and `device_code`, stores only `SHA-256(pepper:*)` in Postgres, returns the cleartext codes to the CLI once.

**2 — Request email code.** CLI sends email + pairing code. API validates the pairing hash, generates a 6-digit OTP, stores its digest, delivers the code to the inbox. CLI gets back a `request_id`.

**3 — Verify.** CLI submits OTP. API compares hashes (timing-safe), runs `upsertUser` (creates account if new), mints session + refresh tokens, persists digests to Postgres and Redis. Response body is `{ ok }` — no tokens yet.

**4 — Poll.** CLI fetches tokens from a Redis staging key via `device_code`. Bearer + refresh travel over HTTPS once.

**5 — Persist locally.** CLI writes `~/.config/agentid/session.json` (`0600`), calls `complete` to delete the Redis staging payload.

Pepper lives in Secret Manager (KMS). From step 5 onward, API calls use `Authorization: Bearer`; the server re-hashes, resolves via Redis, checks revocation in Postgres.

Guards: CLI-bound pairing, 10 min TTL, single use, 5 attempts, rate limits per IP/email/device.

### Git push authorization

Installation token minted only after:

1. Valid session (steps above)
2. User ↔ org access
3. Bot permission row exists
4. Repo + current branch ∈ allow list (`agentid allow`)
5. GitHub App installed on org
6. CLI: workspace org == `git remote` owner

API calls GitHub `POST /app/installations/{id}/access_tokens` scoped to one repo (`contents: write`, `metadata: read`, `pull_requests: write`). Response returned once to the credential helper; not logged, not stored. Denied paths write audit with reason code.

App install callback validates single-use state (10 min TTL), optional expected-org binding, rejects mismatch.

### Lifecycle

| Action | Effect |
|--------|--------|
| `agentid revoke` | Device + sessions revoked in Postgres; local `session.json` deleted |
| Account delete | User, devices, sessions, user-created bots removed |
| Org disconnect | GitHub App uninstalled; org-scoped data wiped |
| Token request | Audit row: org, repo, branch, bot, device, allow/deny |

Browser involvement limited to GitHub App install redirect. No AgentID sessions or GitHub tokens in the browser.

## GitHub App private key (`.pem`)

The GitHub App RSA private key is the only long-lived credential in the system. It never ships to the CLI, never lands in a repo, and never gets baked into a container image.

### Where it lives

| Location | Has the `.pem`? |
|----------|----------------|
| Developer machine / CLI | No |
| Git repository | No (`.pem` in `.gitignore`) |
| Container image | No |
| **Google Secret Manager** | **Yes** — single active version, KMS envelope-encrypted at rest |

On deploy, the API service reads the secret at runtime via a scoped service-account binding (`secretAccessor` on that secret only). The PEM string is held in process memory for the duration of a signing operation, then released.

### What we use it for

The key signs **short-lived RS256 JWTs** (~9 min) that authenticate the AgentID GitHub App to the GitHub API. Those JWTs are used server-side only, to:

- Verify installations and org metadata
- Mint **installation access tokens** (scoped to one repo, ~1h) when an authorized `git push` comes in

The CLI and browser never see the `.pem` or the App JWT. Developers only receive the ephemeral installation token, passed once to Git over the credential-helper pipe.

### How it's protected

- **At rest** — Secret Manager + Cloud KMS. No plaintext on disk anywhere in our infra.
- **In transit** — decrypted inside the Cloud Run instance over the internal Secret Manager API; never sent to clients or logged.
- **At use** — imported into Web Crypto (`RSASSA-PKCS1-v1_5`), used to sign, discarded. Stateless containers; nothing written to a local filesystem.
- **Access control** — one service account, one secret, IAM-scoped. Secret Manager and KMS access shows up in Cloud Audit Logs.
- **Rotation** — generate a new key pair in GitHub App settings, upload new PEM as a new Secret Manager version, redeploy, disable the old GitHub key. No CLI or user action required.

## Requirements

- GitHub CLI (`gh`) for organization discovery and GitHub App installation flow.
- HTTPS Git remotes are recommended.

## License

MIT
