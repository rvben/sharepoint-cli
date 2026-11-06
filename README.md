# sharepoint-cli

Agent-friendly SharePoint Online CLI with JSON output, structured exit codes, and schema introspection.

## Install

### Homebrew

```sh
brew install rvben/tap/sharepoint-cli
```

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

### Nix

With Nix’s `nix-command` and `flakes` features enabled, packages are available
for Linux x64/ARM64 and Apple Silicon macOS:

```sh
nix run github:rvben/sharepoint-cli -- --help
nix build github:rvben/sharepoint-cli
```

Both `Cargo.lock` and `flake.lock` are committed. The package runs the Rust test
suite and checks the installed command and Bash, Fish, and Zsh completions.
Intel Macs are not supported by the pinned nixpkgs; use the other installation
methods above.

For NixOS or Home Manager, add the input to your flake:

```nix
inputs.sharepoint-cli = {
  url = "github:rvben/sharepoint-cli";
  inputs.nixpkgs.follows = "nixpkgs";
};
```

Then add `inputs.sharepoint-cli.packages.${pkgs.stdenv.hostPlatform.system}.default`
to `environment.systemPackages` (NixOS) or `home.packages` (Home Manager).
Pass `inputs` through `specialArgs` for `nixosSystem` or `extraSpecialArgs` for
`homeManagerConfiguration`. The overlay `inputs.sharepoint-cli.overlays.default`
also provides `pkgs.sharepoint-cli` using your package set and Rust toolchain overrides.
Following your own nixpkgs uses its toolchain; it must meet the Rust version
required by `Cargo.toml` and support your platform.

For development:

```sh
nix develop
nix flake check                  # build, Rust tests, installed-command checks
nix fmt -- --check flake.nix nix/*.nix
```

`direnv allow` is optional and requires nix-direnv. The development shell
includes the package's native build dependencies and Rust development tools.
Update Nix inputs deliberately with `nix flake update`, review the lockfile,
and run the checks before committing it.

## Quick start

```sh
# First-time setup: configures tenant + default site, then signs in
sharepoint init

# Or sign in to an already-configured profile
sharepoint auth login

# List sites: followed sites (graph backend) or all sites via search (rest backend)
sharepoint sites list

# List libraries in a site
sharepoint drives list <site>

# Browse a library
sharepoint files ls <site>:<library>/

# Stat / download / search
sharepoint files stat   <site>:<library>/path/to/file
sharepoint files download <site>:<library>/path/to/file --path ./out.bin
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
- Errors go to stderr; JSON errors have the shape
  `{"error":{"kind":"not_found","message":"…","exit_code":4}}`.
  `--quiet` keeps errors visible. Exit codes are 0 (success), 1 (transport/local
  failure), 2 (invalid input/read-only), 3 (authentication/access denied),
  4 (not found), 5 (API failure), 6 (rate limit), and 7 (conflict).

List responses contain `items`, `total`, and an opaque `next` cursor. Pass a
non-null cursor back with `--page`; stop when `next` is null. `--limit` must be
positive and sets an upper bound, so a server page or a search filtered by
`--name` can return fewer items. Use `--all` for sites, drives, file listings,
and search to fetch all remaining pages. `files ls --recursive` returns the
complete tree and cannot be combined with pagination flags.
For sites and files, `total` counts the returned items. For drives and Graph
cached accounts, it counts the complete collection before limiting.
`files find` searches the referenced folder and its subfolders; reference the
library root to search the whole library.
Graph `auth status` supports `--limit` and `--page`; REST status describes a
single host session and rejects `--page`.

Downloads use `--path FILE`, with parent directories created as needed. A
completed transfer replaces an existing destination only with `--overwrite`;
transfer errors leave the destination file unchanged and remove their staging
file. Parent directories created for a download remain. Abrupt process termination
can leave a `.sharepoint-download-*` staging file beside the destination.
`--overwrite` replaces the file entry (including a symlink), rather than writing
through it; existing metadata and hard links are not preserved. On Unix,
downloaded files are private to their owner.
Without `--overwrite`, a file created during the transfer is also protected.
`--path -` writes raw file bytes to stdout, including in JSON mode; stream
failures can leave partial bytes on stdout, with the error on stderr.
The legacy `--output FILE` and `-o FILE` aliases work **after** `files download`.
Put the global `--output json|text|auto` **before** `files download`, and prefer
`--path` for an unambiguous destination.

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

Run `sharepoint config path` to print the resolved config location. By default,
configuration and cached credentials use the native directories for your OS:

| OS | Configuration | Cached credentials |
| --- | --- | --- |
| Linux | `~/.config/sharepoint/` | `~/.cache/sharepoint/` |
| macOS | `~/Library/Application Support/sharepoint/` | `~/Library/Caches/sharepoint/` |
| Windows | `%APPDATA%\sharepoint\` | `%LOCALAPPDATA%\sharepoint\` |

Set `XDG_CONFIG_HOME` or `XDG_CACHE_HOME` to an absolute directory to override
these defaults on any OS. The CLI adds the `sharepoint` subdirectory. Empty or
relative overrides are ignored. Config is saved as `config.toml`; cached Graph
tokens and REST sessions use `tokens.json` and `sessions.json`, respectively.

For the default **Graph backend**, supply two things before signing in:

- A **tenant** (your Microsoft 365 domain or tenant GUID).
- A **client_id** for an Entra public-client app you've registered. The app needs the device-code flow enabled and delegated `Files.Read.All`, `Sites.Read.All`, and `offline_access` scopes. `sharepoint init` walks you through saving both into the active profile.

The **REST backend** needs a SharePoint host and browser sign-in, with no tenant
or client ID. See [browser-session setup](#without-microsoft-graph-browser-session).

Environment overrides:

- `SHAREPOINT_PROFILE` — active profile name
- `SHAREPOINT_TENANT_ID` — tenant override
- `SHAREPOINT_CLIENT_ID` — Entra application (client) ID — **required** for the `graph` backend
- `SHAREPOINT_BACKEND` — `graph` (default) or `rest`
- `SHAREPOINT_HOST` — SharePoint host for the `rest` backend, e.g. `contoso.sharepoint.com`
- `SHAREPOINT_COOKIE` — session cookie header for the `rest` backend; skips the stored browser session
- `SHAREPOINT_BROWSER` — browser executable that `auth login` launches for the `rest` backend

## Without Microsoft Graph (browser session)

If you can't register an Entra app or get Graph consent, use the `rest`
backend. It signs in through a real browser and calls the SharePoint REST API
(`/_api`) with your session cookies. No tenant or client ID is needed.

```toml
# Add to the file printed by `sharepoint config path`
[profile.work]
backend = "rest"
sharepoint_host = "contoso.sharepoint.com"
```

```sh
sharepoint --profile work auth login     # opens Edge/Chrome/Chromium; finish SSO
sharepoint --profile work auth status
sharepoint --profile work sites list
sharepoint --profile work files ls https://contoso.sharepoint.com/sites/Marketing/Documents/
sharepoint --profile work doctor
```

- `auth login` needs a Chromium-based browser (Edge, Chrome, Chromium or Brave).
  It launches it with a throwaway profile, waits up to five minutes for you to
  finish signing in, then closes it. Set `SHAREPOINT_BROWSER` to pick one
  explicitly.
- A snap-packaged Chromium (the default `chromium` on Ubuntu) won't work: its
  private `/tmp` hides the browser's DevTools port file from the CLI. Point
  `SHAREPOINT_BROWSER` at a non-snap Chrome, Chromium or Edge instead.
- The session is stored per host in `sessions.json` in the cache directory
  above, with mode 0600 on Unix. When it expires, run
  `sharepoint auth login` again; `sharepoint auth logout` removes it.
- OneDrive lives on its own host (`contoso-my.sharepoint.com`), so it needs
  its own profile and `auth login`:
  `SHAREPOINT_HOST=contoso-my.sharepoint.com sharepoint drives list https://contoso-my.sharepoint.com/personal/jane_contoso_com`
  lists its `Documents` library with `drive_type` `business`, as Graph does.
- `sites list` without a query returns all sites visible to you via search
  (the `graph` backend returns followed sites).
- `files stat` has no `download_url` on this backend.
- Tenants that require a compliant or managed device may block sign-in from a
  fresh browser profile.
- For CI, pass `SHAREPOINT_COOKIE="FedAuth=…; rtFa=…"` instead of signing in.

## License

MIT

## Testing

Run `make test-simulator` for repeatable local REST smoke tests with dummy data,
or `make simulator` to explore the fixtures interactively. No tenant or login is
needed. See [the testing guide](docs/testing.md) for scenarios, custom fixtures,
and live SharePoint/OneDrive testing.

## Releasing

Vership owns versioning, changelog generation, release commits, and tags. See
[the release runbook](docs/releases.md) for the verified workflow and recovery policy.
