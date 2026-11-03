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
