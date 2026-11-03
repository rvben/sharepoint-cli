# Testing

## Automated checks

Run `make check` for formatting, Clippy, and the Rust tests. CI runs unit and
integration tests on Linux, macOS, and Windows. The REST integration tests use
local HTTP servers and temporary config/cache directories; they need no tenant,
browser, or credentials.

To run just the REST tests and the config-directory regression test:

```sh
cargo nextest run --test 'cmd_rest_*' --test rest_client --test cmd_config
```

## Reusable local simulator

The simulator runs a read-only subset of SharePoint REST on `127.0.0.1`. It uses
dummy cookies and bundled data, with no tenant, subscription, browser, or real
credentials. It uses the existing development dependencies; the shipped
`sharepoint` binary has no simulator dependency or additional commands.

Run the complete smoke suite against a local build:

```sh
make test-simulator
```

Or select any built or downloaded CLI binary explicitly (Windows binaries have
an `.exe` extension):

```sh
cargo build --bin sharepoint
cargo run --example simulator -- smoke --cli target/debug/sharepoint
```

`smoke` starts disposable servers on available ports, verifies the selected
binary's config/cache/session paths are inside temporary directories, and
removes those directories afterward. It strips
inherited SharePoint/Microsoft settings from each CLI subprocess and preserves
the operating system environment. Each CLI call has a 30-second timeout. A
failed check exits nonzero. The same suite runs through `tests/simulator.rs`
in the existing Linux, macOS, and Windows CI matrix.

An older binary that cannot honor directory isolation is rejected with a clear
error. In particular, Windows releases before the cross-platform XDG fix need
an updated build. After building the example, `target/debug/examples/simulator`
(with `.exe` on Windows) also runs directly and can be copied elsewhere; its
default fixtures are embedded in the executable.

Checks cover identity and doctor, site discovery, SharePoint and OneDrive
libraries, folder traversal, empty folders, continuation cursors, library-scoped
search, metadata, missing files/sessions, escaped filenames, exact download
bytes, authentication failures, and retry success/exhaustion. Five additional
checks exercise sanitized response shapes from pinned public PnP Core SDK
recordings; see the [corpus provenance and limitations](../examples/simulator/contracts/README.md).

### Interactive use

Start the server in one terminal:

```sh
cargo run --example simulator -- serve
```

The default port is 8787. Use `--port 0` to select an available port or
`--port 9000` for a fixed alternative. The server prints its address and dummy
connection settings. Stop it with Ctrl-C.

In another terminal, use a subshell with temporary config/cache directories:

```sh
(
  simulator_home=$(mktemp -d)
  trap 'rm -rf "$simulator_home"' EXIT
  export XDG_CONFIG_HOME="$simulator_home/config"
  export XDG_CACHE_HOME="$simulator_home/cache"
  export SHAREPOINT_PROFILE=simulator
  export SHAREPOINT_BACKEND=rest
  export SHAREPOINT_HOST=simulator.sharepoint.com
  export SHAREPOINT_REST_ENDPOINT=http://127.0.0.1:8787
  export SHAREPOINT_COOKIE='FedAuth=simulator; rtFa=simulator'

  sharepoint init --no-login --read-only </dev/null
  sharepoint auth status
  sharepoint doctor
  sharepoint sites list --all
  sharepoint drives list https://simulator.sharepoint.com/sites/Marketing
  sharepoint files ls https://simulator.sharepoint.com/sites/Marketing/Documents --recursive
  sharepoint files find https://simulator.sharepoint.com/sites/Marketing/Documents --query txt --all
  sharepoint files stat https://simulator.sharepoint.com/sites/Marketing/Documents/hello.txt
  sharepoint files download https://simulator.sharepoint.com/sites/Marketing/Documents/hello.txt --path -
)
```

Use `./target/debug/sharepoint` in place of `sharepoint` if it is not installed.
Keep `SHAREPOINT_REST_ENDPOINT` set: the SharePoint-looking fixture URLs identify
data; all REST requests go to the local server. Use its printed port if different.
There is no `auth login` step; the dummy cookie supplies the simulated session.

For PowerShell, start a fresh terminal and set connection settings this way:

```powershell
$simulatorHome = Join-Path ([System.IO.Path]::GetTempPath()) ([guid]::NewGuid().ToString())
$env:XDG_CONFIG_HOME = Join-Path $simulatorHome 'config'
$env:XDG_CACHE_HOME = Join-Path $simulatorHome 'cache'
$env:SHAREPOINT_PROFILE = 'simulator'
$env:SHAREPOINT_BACKEND = 'rest'
$env:SHAREPOINT_HOST = 'simulator.sharepoint.com'
$env:SHAREPOINT_REST_ENDPOINT = 'http://127.0.0.1:8787'
$env:SHAREPOINT_COOKIE = 'FedAuth=simulator; rtFa=simulator'
```

Run the CLI commands above (omit `</dev/null` in PowerShell). When finished,
remove the temporary directory with `Remove-Item -Recurse -Force $simulatorHome`
and close that terminal to discard its environment changes.

### Fixtures and failure scenarios

The bundled [JSON fixture](../examples/simulator/fixtures.json) has Marketing
and Engineering sites and a personal OneDrive site. Marketing has visible
`Documents` and `Archive` libraries, a hidden library, and an ordinary Tasks list.
The CLI must exclude the latter two. `Documents` includes `hello.txt`, `Plans/Q4 plan.txt`,
an empty folder, the hidden `Forms` folder, and `O'Brien #1 %.txt`. Full URLs
must encode reserved characters, for example
`https://simulator.sharepoint.com/sites/Marketing/Documents/O%27Brien%20%231%20%25.txt`.
OneDrive lives at `https://simulator-my.sharepoint.com/personal/simulator`;
set `SHAREPOINT_HOST=simulator-my.sharepoint.com` when using that fixture.

Copy the fixture and pass `serve --fixtures /path/to/fixtures.json` to customize
it. Sites define `id`, `name`, `host`, `path`, and `libraries`. Libraries define
`id`, `name`, `root` (relative to the site), a positive `template`, and optional
`hidden`, `folders`, and `files`. Templates 101 and 700 represent document and
OneDrive libraries; other templates exercise exclusion of ordinary lists.
Files define a library-relative `path` and UTF-8 `content`.
Intermediate folders are inferred, and metadata lengths count UTF-8 bytes.
Content stays in memory; fixture paths never read from the local filesystem.
Restart to reload fixtures or reset a scenario. The smoke suite always uses
the bundled model and recording fixtures so its expectations remain reproducible.

An optional top-level `faults` array injects failures for decoded server-relative
resource paths. Each entry requires `resource`, a 4xx/5xx `status`, and `message`;
optional `operation` selects `metadata`, `list`, or `download`, and `recursive`
extends the rule to descendants at path boundaries. Rules are checked in order;
the first match wins. For example:

```json
{
  "resource": "/sites/Marketing/Archive/locked.txt",
  "operation": "download",
  "status": 403,
  "message": "Download blocked for this resource."
}
```

This rule is bundled: `files stat` for `Archive/locked.txt` works, its download
fails, and `Archive/old.txt` remains downloadable. Resource fault injection does
not implement SharePoint's permission inheritance or search security trimming.

```sh
cargo run --example simulator -- serve --scenario expired
cargo run --example simulator -- serve --scenario throttle-once --page-size 2
```

| Scenario | Behavior for authenticated API requests |
| --- | --- |
| `healthy` | Serve fixture data. |
| `expired` | Return 401; CLI exits 3 with a sign-in hint. |
| `forbidden` | Return 403; CLI exits 3. |
| `redirect` | Return 302 to a local sign-in placeholder; CLI exits 3 without following it. |
| `throttled` | Always return 429; CLI exits 6 after retries. |
| `throttle-once` | Return 429 for the first request, then serve data. |
| `transient` | Return 503 for the first request, then serve data. |
| `unavailable` | Always return 503; CLI exits 5 after retries. |

Retry scenarios send `Retry-After: 0` to keep smoke tests fast. Search responses
and REST collections contain at most one row by default; `--page-size` changes
that cap. Collection responses expose `odata.nextLink`; continuation URLs retain
the original filters and selected fields. The CLI follows collection pages
before producing its own item cursors.
Search supports case-insensitive substring matching and `*`, with the site
or library scope generated by this CLI. It does not implement general KQL.
Unsupported routes return 404, unsupported methods 405, malformed search
requests 400, and missing/wrong dummy cookies 401. An unauthenticated
`GET /_simulator/health` is available for readiness checks.

The simulator accepts `application/json;odata=nometadata` only; a missing or
unsupported `Accept` header returns 406 rather than silently supplying a format
that SharePoint might not return. Unknown/duplicate query parameters and invalid
selected properties or expansions return 400. `$select` projects the response;
`$expand=RootFolder` enables library root properties. `$filter` supports typed
`eq` comparisons on scalar properties, `and`, `or`, and parentheses with normal Boolean precedence.
Unsupported operators fail explicitly. `$top` caps the complete collection;
server paging uses opaque `$skiptoken` values. This is a bounded subset of
[SharePoint's OData query operations](https://learn.microsoft.com/en-us/sharepoint/dev/sp-add-ins/use-odata-query-operations-in-sharepoint-rest-requests).

### Replaying independently captured responses

The bundled [PnP Core SDK corpus](../examples/simulator/contracts/README.md)
adapts public server-response recordings with pinned URLs and SHA-256 values.
The CLI smoke suite checks identity, metadata, search paging, null cells, and
not-found errors against these response shapes. Paths and queries are adapted,
and the web/library setup responses are synthetic. This is independent parser
evidence, not a live run of this CLI or proof that Microsoft accepts its exact
requests.

When a tenant becomes available, save sanitized JSON responses from actual CLI
requests in a recording fixture and run `serve --fixtures recording.json`.
Keep original captures outside Git. Replace real identities, tenant/site names,
and sensitive document content before creating a reusable fixture; retain field
types, response envelopes, error shapes, and continuation structure. Record the
capture date, environment, and response format in `source`.

The following is a **synthetic format example**, not a tenant capture:

```json
{
  "recording": {
    "host": "simulator.sharepoint.com",
    "source": "Synthetic format example; replace with capture provenance",
    "exchanges": [
      {
        "path": "/_api/web/currentuser",
        "query": {"$select": "Title,Email,LoginName"},
        "status": 200,
        "body": {
          "Title": "Test User",
          "Email": "test@example.test",
          "LoginName": "test@example.test"
        }
      }
    ]
  }
}
```

Each exchange matches an exact URL-encoded `path` and a complete, decoded
`query` map (query ordering is irrelevant). Missing exchanges return 404; there
is no fallback to generated data. `status` and JSON `body` are replayed directly.
Optional response `headers` support `Content-Type` and `Retry-After` only. Request
headers and authentication cookies are never part of the recording format.
Duplicate request matches and mixing a recording with generated `sites` or
resource `faults` are rejected.

Replace the origin in recorded `odata.nextLink`, `@odata.nextLink`, or `__next`
values with `{{endpoint}}` so continuations target the simulator's current port.
Relative continuations can remain relative. The CLI rejects foreign origins
and repeated continuation URLs. A small synthetic test covers the replay
mechanism; the smoke suite also uses the independent upstream response corpus.
Capturing complete exchanges from this CLI against a tenant remains a separate step.

The expanded paging checks expose a truncation bug in v0.0.13 and earlier:
those binaries stop after the first collection response. Use the current build
for the full smoke suite; an older binary failing that check is intentional.

The simulator validates generated scenarios and the imported response shapes.
Exact live request compatibility, browser login, MFA, Conditional Access,
permissions, and Microsoft Graph still need a tenant for end-to-end validation.

## Live REST smoke test

Use a dedicated Microsoft 365 test tenant with a sample document library and
a small file whose contents you know. A
[Microsoft 365 developer sandbox](https://learn.microsoft.com/en-us/office/developer-program/microsoft-365-developer-program-get-started)
includes SharePoint and OneDrive, subject to eligibility, and requires a valid
billing account. The developer sandbox itself is free. An alternative is a
[Microsoft 365 Business trial](https://learn.microsoft.com/en-us/microsoft-365/commerce/try-or-buy-microsoft-365?view=o365-worldwide);
it lasts 30 days and normally converts to a paid annual subscription unless
recurring billing is turned off.

Install the published version you want to test and check `sharepoint --version`.
Find the config file with `sharepoint config path`, then add a separate profile
using your test tenant's host:

```toml
[profile.smoke]
backend = "rest"
sharepoint_host = "contoso.sharepoint.com"
```

Replace the example host, site, library, and file references below with your
test data. Finish browser sign-in when prompted. These commands read tenant
data; the download writes a local file.

```sh
sharepoint --profile smoke auth login
sharepoint --profile smoke auth status
sharepoint --profile smoke doctor
sharepoint --profile smoke sites list
sharepoint --profile smoke drives list https://contoso.sharepoint.com/sites/Testing
sharepoint --profile smoke files ls https://contoso.sharepoint.com/sites/Testing/Documents/
sharepoint --profile smoke files stat https://contoso.sharepoint.com/sites/Testing/Documents/smoke.txt
sharepoint --profile smoke files download https://contoso.sharepoint.com/sites/Testing/Documents/smoke.txt --path ./sharepoint-smoke.txt
```

Check that auth status reports a verified identity, doctor reports healthy,
the expected library and file are listed, and the downloaded bytes match the
fixture. A smoke test is successful only when those checks pass against the
published binary; passing the local mock tests alone does not validate SSO.

For OneDrive, add another profile with the tenant's `-my.sharepoint.com` host,
sign in separately, and list the personal site's drives. Its Documents library
should report `drive_type` as `business`.

The automated REST tests cover rejected/expired sessions and the instruction
to sign in again. To check live recovery after a session expires, repeat
`auth status`, confirm the authentication error, then run `auth login` and
verify that `auth status` succeeds. Session storage is shared by profiles using
the same host, so use a dedicated test tenant for this exercise.
