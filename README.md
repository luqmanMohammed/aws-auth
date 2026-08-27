# aws-auth

A CLI for getting AWS credentials out of IAM Identity Center (AWS SSO) and into
whatever needs them — your shell, a single command, `kubectl`, or a script that has
to touch many accounts at once.

It keeps one SSO session in a local cache and exchanges it for short-lived role
credentials on demand, so you sign in through the browser about as often as your
Identity Center session duration requires and not once per command.

## Install

Download a build from the [releases page](../../releases). Artifacts are published for
Linux x64 and arm64 (statically linked against musl), macOS arm64, and Windows x64.

To build from source you need Rust 1.88 or later:

```sh
cargo build --release
# target/release/aws-auth
```

## Getting started

Point it at your Identity Center portal once:

```sh
aws-auth init --sso-start-url https://my-company.awsapps.com/start --sso-region eu-west-2
```

That writes `~/.aws-auth/config.json`. Then use any of the commands below; the first
one opens a browser to authorize the device, and later ones reuse the cached session.

```sh
# run one command with credentials
aws-auth exec -a 111111111111 -r AdminRole -- aws s3 ls

# load credentials into the current shell
eval "$(aws-auth eval -a 111111111111 -r AdminRole)"

# save some typing
aws-auth alias set prod -a 111111111111 -r AdminRole
aws-auth exec -A prod -- aws s3 ls
```

## Commands

| Command | What it does |
| --- | --- |
| `init` | Write or update `config.json` |
| `eval` | Print `export AWS_…` lines (or JSON) for shell evaluation |
| `exec` | Run a command with credentials in its environment |
| `eks` | Print a Kubernetes `ExecCredential` for an EKS cluster |
| `alias` | `set` / `unset` / `list` short names for an account and role |
| `sso` | `list-accounts` / `list-account-roles` available to you |
| `batch exec` | Run a command across many accounts, optionally in parallel |
| `unlock` | Clear the create-token lock (see [Rate limiting](#rate-limiting)) |
| `logout` | End the SSO session and clear the local cache |

Every credential-taking command accepts either `-a <account-id> -r <role>` or
`-A <alias>`, plus:

| Flag | Meaning |
| --- | --- |
| `-C`, `--config-dir` | Config directory. Also read from `AWS_AUTH_CONFIG_DIR`. Defaults to `~/.aws-auth` |
| `-R`, `--region` | AWS region for the credentials. Defaults to `eu-west-2` |
| `-t`, `--refresh-sts-token` | Ignore the cached role credentials and fetch new ones |
| `-i`, `--ignore-cache` | Discard the SSO client registration too, forcing a new device authorization |

`--help` on any subcommand lists the rest.

A short flag means the same long flag in every command, so `-o` is always `--output` and
never `--output-dir`. Where two names want the same letter the second takes the uppercase
form — `-a`/`-A` account/alias, `-r`/`-R` role/region, `-c`/`-C` cluster/config-dir,
`-o`/`-O` output/omit-fields, `-f`/`-F` filter/fail-fast, `-d`/`-D` debug/output-dir. The
list forms `batch` takes share the letter of their singular, so `-a` is `--account-ids`
there. `init` configures a machine once and is long-only apart from `-C`.

### eval

```sh
eval "$(aws-auth eval -A prod)"          # export AWS_… (PowerShell $env:… on Windows)
aws-auth eval -A prod --output json      # machine readable
```

Sets `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN`, `AWS_REGION`,
`AWS_DEFAULT_REGION` and `AWS_SSO_SESSION_EXPIRATION`.

### exec

```sh
aws-auth exec -A prod -- terraform apply
```

The child inherits stdin, stdout and stderr, and **aws-auth exits with the child's
exit status** — so `&&`, `set -e` and CI steps behave as you'd expect. A child killed
by a signal is reported as `128 + signal`, the same as a shell does.

### eks

Prints a `client.authentication.k8s.io/v1beta1` `ExecCredential`, so it can be used
directly as a kubeconfig credential plugin:

```yaml
users:
  - name: my-cluster
    user:
      exec:
        apiVersion: client.authentication.k8s.io/v1beta1
        command: aws-auth
        args: ["eks", "-A", "prod", "-c", "my-cluster", "-R", "eu-west-2"]
```

Tokens are cached per account, role, region and cluster under `<config-dir>/eks`, and
entries left untouched for seven days are cleaned up. `--eks-expiry-seconds` tunes the
token lifetime (1 to 604800, default 860).

### batch exec

```sh
# every account you can see, trying each role in order until one works
aws-auth batch exec -r AdminRole,ReadOnly -p 8 -- aws sts get-caller-identity

# a subset, by id, alias, or account-name regex
aws-auth batch exec -a 111111111111,222222222222 -r AdminRole -- aws s3 ls
aws-auth batch exec -A prod,staging -- aws s3 ls
aws-auth batch exec -f '^prod-' -r AdminRole -- aws s3 ls

# stop dispatching accounts once one of them fails
aws-auth batch exec -F -A prod,staging -- ./migrate.sh
```

Each child additionally gets `AWS_ACCOUNT_ID`. Accounts that resolve under no role are
reported on stderr, and the command fails if none resolved at all. `-D <dir>` writes
per-account `*-stdout.log` / `*-stderr.log`; `-s` discards output; `-d` adds progress
logging.

**Any account whose command fails makes aws-auth exit non-zero**, and each failure is
named on stderr — unlike `exec`, the exit status is a pass/fail for the run rather than
a child's own code, since there are many children. `-F` / `--fail-fast` stops dispatching
the accounts that have not started yet, reporting them as skipped; accounts already in
flight are allowed to finish. It changes what runs, not what the run exits with.

### Output formats

`alias list`, `sso list-accounts` and `sso list-account-roles` take `-o json|text`,
`-H` to drop headers, and `-O` to omit columns. Column names are matched ignoring case
and spaces, so `-O accountId`, `-O "Account Id"` and `-O accountid` are equivalent, and
an unrecognised name is an error rather than being ignored.

## Configuration

`config.json` in the config directory:

```json
{
  "startURL": "https://my-company.awsapps.com/start",
  "ssoRegion": "eu-west-2",
  "maxAttempts": 10,
  "initialDelay": { "secs": 10, "nanos": 0 },
  "retryInterval": { "secs": 5, "nanos": 0 },
  "createTokenRetryThreshold": 5,
  "createTokenLockDecay": [7200, 0],
  "noBrowser": false
}
```

Only `startURL` and `ssoRegion` are required, and they must be non-empty. `maxAttempts`
must be at least 1. `createTokenLockDecay` may not be negative — use `0` to keep a lock
until `aws-auth unlock` clears it. An invalid value is rejected on load rather than
silently ignored. Set any of them with `aws-auth init --update`, for example:

```sh
aws-auth init --update --max-attempts 20 --no-browser true
```

Alongside it live `aliases.json` (your aliases), `cache.json` (the SSO session and
cached role credentials), `aws-sso-create-token-lock.json`, and `eks/`.

## Headless hosts

Device authorization prints the user code and verification URL before trying to open a
browser, and carries on if no browser can be opened — so on a machine without one you
can authorize from elsewhere. In that case it polls for as long as the verification
code is valid rather than the shorter interactive window. Set `"noBrowser": true` (or
`aws-auth init --update --no-browser true`) to skip the attempt entirely.

## Rate limiting

Repeatedly failing device authorizations can get your IP blocked by AWS, so aws-auth
counts consecutive failures and refuses to try again once
`createTokenRetryThreshold` is reached. The lock clears itself after
`createTokenLockDecay`, or immediately with:

```sh
aws-auth unlock
```

Set `createTokenRetryThreshold` to `0` to disable locking, or `createTokenLockDecay`
to `0` to make locks permanent until unlocked by hand.

## Notes on behaviour

- **stdout carries only the payload.** Credentials, JSON and tables go to stdout;
  status messages, warnings and errors go to stderr. Capturing stdout is always safe.
- **Exit codes.** `0` on success, `1` on failure, `2` for a command line error, and the
  child's own status for `exec`.
- **File permissions.** On Unix the config directory and `eks/` are created `0700`, and
  `cache.json` and cached EKS tokens are written `0600` — they hold live credentials.
  Windows has no equivalent handling and inherits the directory's ACL.
- **Cache writes are atomic**, so a concurrent reader never sees a half-written file.

## Development

```sh
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

Tests are unit tests inside each module and need no network or AWS account.

## Licence

[MIT](LICENSE)
