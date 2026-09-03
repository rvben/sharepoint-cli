# sharepoint-cli

Agent-friendly SharePoint Online CLI with JSON output, structured exit codes, and schema introspection.

## Install

### From crates.io

```sh
cargo install sharepoint-cli
```

### From PyPI

```sh
pip install sharepoint-cli-rs
# or
uv tool install sharepoint-cli-rs
```

### From source

```sh
git clone https://github.com/rvben/sharepoint-cli
cd sharepoint-cli
cargo install --path .
```

## Quick start

```sh
# First-time setup: configures tenant + default site, then signs in
sharepoint init

# Or sign in to an already-configured profile
sharepoint auth login

# List followed sites
sharepoint sites list

# List libraries in a site
sharepoint drives list <site>

# Browse a library
sharepoint files ls <site>:<library>/

# Stat / download / search
sharepoint files stat   <site>:<library>/path/to/file
sharepoint files download <site>:<library>/path/to/file -o ./out.bin
sharepoint files find   <site>:<library>/ --name '*.pdf'
sharepoint doctor
```

For a headless or staged setup, provide the tenant and public-client application
ID explicitly and defer sign-in:

```sh
sharepoint init --tenant contoso.onmicrosoft.com \
  --client-id 00000000-0000-0000-0000-000000000000 \
  --default-site Marketing --read-only --no-login
sharepoint auth login
```

Interactive setup reuses existing profile values as defaults. Non-interactive
setup never reads prompts from piped stdin; missing values produce an actionable
input error instead.

## Output

- Human output on stdout, status messages on stderr.
- `--json` (or non-TTY stdout) emits machine-readable JSON on stdout.
- `--quiet` suppresses status messages.
- `--no-color` and the standard `NO_COLOR` environment variable disable ANSI output.

Suite-level discovery is available without guessing: use `sharepoint doctor --offline`, `sharepoint schema --command 'files stat'`, `sharepoint config show`, and `sharepoint completions <shell>`.

Authentication and profile management follow the same suite-wide commands:

```sh
sharepoint auth status
sharepoint auth status --offline
sharepoint profile list
sharepoint profile use work
sharepoint profile remove old --yes
sharepoint config path
```

## Configuration

Config lives at `$XDG_CONFIG_HOME/sharepoint/config.toml` (or `~/.config/sharepoint/config.toml`). Run `sharepoint config path` to print the resolved location.

You must supply two things before signing in:

- A **tenant** (your Microsoft 365 domain or tenant GUID).
- A **client_id** for an Entra public-client app you've registered. The app needs the device-code flow enabled and delegated `Files.Read.All`, `Sites.Read.All`, and `offline_access` scopes. `sharepoint init` walks you through saving both into the active profile.

Environment overrides:

- `SHAREPOINT_PROFILE` — active profile name
- `SHAREPOINT_TENANT_ID` — tenant override
- `SHAREPOINT_CLIENT_ID` — Entra application (client) ID — **required**

## License

MIT

## Releasing

Vership owns versioning, changelog generation, release commits, and tags. See
[the release runbook](docs/releases.md) for the verified workflow and recovery policy.
