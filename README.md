# sharepoint-cli

Agent-friendly SharePoint Online CLI with JSON output, structured exit codes, and schema introspection.

## Install

### From crates.io

```sh
cargo install sharepoint-cli
```

### From PyPI

```sh
pip install sharepoint-cli
# or
uv tool install sharepoint-cli
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
```

## Output

- Human output on stdout, status messages on stderr.
- `--json` (or non-TTY stdout) emits machine-readable JSON on stdout.
- `--quiet` suppresses status messages.

## Configuration

Config lives at `$XDG_CONFIG_HOME/sharepoint/config.toml` (or `~/.config/sharepoint/config.toml`). Run `sharepoint config path` to print the resolved location.

Environment overrides:

- `SHAREPOINT_PROFILE` — active profile name
- `SHAREPOINT_TENANT_ID` — tenant override
- `SHAREPOINT_CLIENT_ID` — Azure AD application ID

## License

MIT
