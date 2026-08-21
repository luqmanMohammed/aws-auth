# Changelog

## Unreleased

### Security

- Credential caches are written owner-only (`0600`), and directories aws-auth creates
  are `0700`. Files left behind by earlier versions are tightened on the next write.
  Unix only; Windows inherits the directory's ACL.
- Cache writes are atomic, so a concurrent reader can no longer see a half-written
  `cache.json`. A path that is a symlink is written through rather than replaced.
- An unlocatable home directory is an error instead of silently falling back to a
  world-writable temp directory.
- CI grants `contents: read` to the build matrix; only the release job can write.

### Added

- `noBrowser` config setting, for hosts that have no browser to open.
- Expired EKS tokens are pruned once they have been untouched for seven days.
- `--omit-fields` rejects names that match no column instead of ignoring them.

### Fixed

- `init --update` and `init --recreate` did nothing at all; the condition made both
  flags unreachable.
- `init` validates its inputs before touching the config directory. `--recreate`
  without a start URL and region used to delete the directory and only then fail.
- `init` honours `AWS_AUTH_CONFIG_DIR`, which every other command already did.
- `exec` exits with the child's status, so `&&`, `set -e` and CI steps see failures.
  A signalled child reports `128 + signal`.
- A missing or malformed config is reported instead of panicking, so the message
  explaining how to run `init` is actually shown.
- An SSO scope is requested at registration, which is what makes Identity Center
  issue a refresh token. Access tokens are now renewed silently rather than needing a
  browser at every expiry, and clients registered by older versions re-register
  themselves on the next device authorization.
- An expired access token no longer discards the refresh token alongside it.
- A rejected access token is re-acquired once per run before failing.
- A freshly obtained SSO token is kept even when the credential fetch that triggered
  it fails, instead of forcing another browser sign-in.
- Device authorization prints the verification URL and continues when no browser can
  be opened, and then polls for as long as the code is valid.
- `batch exec` fails when no credentials resolved, and reports skipped accounts
  without needing `--debug`.
- `batch --role-order` splits on commas, like `--account-ids` and `--aliases`.
- `batch --parallel 0` is rejected rather than panicking.
- `--omit-fields` matches column names in either output format, ignoring case and
  spacing.
- `sso list-accounts` and `sso list-account-roles` tolerate accounts with fields the
  API left unset.
- A zero `createTokenRetryThreshold` disables locking, as documented.
- Lock provider errors are reported instead of hitting a `todo!()`.
- Remaining panics on unexpected values removed, including the credential expiry
  conversion, duration conversions, and short rows in text output.
- `--eks-expiry-seconds` is bounded to the range AWS accepts.
- `AWS_SESSION_TOKEN` is left unset rather than exported empty when there is no token.

### Changed

- Errors print as plain text on stderr with a proper exit code, rather than a
  quoted `Debug` string. `1` for failures, `2` for command line errors.
- Status messages go to stderr, leaving stdout for payload only.
- `init` no longer advertises short flags that mean something else in other
  subcommands; the old letters still work. `-C` is now the documented config-dir flag.
- Runs on a single-threaded async runtime, since parallelism comes from the worker
  pool rather than tokio tasks. Polling sleeps no longer block the runtime.
- Release builds use link time optimisation and are stripped, roughly halving the
  binary.
- `tokio` is declared with only the features this crate uses.

### Internal

- Minimum supported Rust version declared as 1.88.
- Test suite added, covering file permissions, locking, cache resolution, the `init`
  flag matrix, EKS token caching and pruning, exit codes and output formatting. The
  tests were written by AI and are marked as unreviewed.
- CI lints with `clippy --all-targets -- -D warnings` and runs tests before the
  release build.
