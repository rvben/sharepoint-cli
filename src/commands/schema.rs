//! `sharepoint schema` - machine-readable description of all commands.
//!
//! This command must work with no authentication, no config file, and no
//! network access. It is the first thing an agent calls when it knows nothing
//! about the tool.

use serde_json::json;

pub fn run() -> crate::error::Result<()> {
    let schema = json!({
        "clispec": "0.2",
        "name": "sharepoint",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Agent-friendly SharePoint Online CLI with JSON output, structured exit codes, and schema introspection",
        "global_args": [
            {
                "name": "--output",
                "type": "string",
                "enum": ["auto", "text", "json"],
                "default": "auto",
                "description": "Output format. auto emits JSON when stdout is not a TTY; text always emits human-friendly output; json always emits JSON."
            },
            {
                "name": "-o",
                "type": "string",
                "enum": ["auto", "text", "json"],
                "default": "auto",
                "description": "Short alias for --output."
            },
            {
                "name": "--quiet",
                "type": "boolean",
                "default": false,
                "description": "Suppress informational messages on stderr."
            },
            {
                "name": "--profile",
                "type": "string",
                "required": false,
                "description": "Active config profile. Env: SHAREPOINT_PROFILE."
            },
            {
                "name": "--tenant",
                "type": "string",
                "required": false,
                "description": "Tenant ID or domain override. Env: SHAREPOINT_TENANT_ID."
            },
            {
                "name": "--client-id",
                "type": "string",
                "required": false,
                "description": "Azure app client ID override. Env: SHAREPOINT_CLIENT_ID."
            }
        ],
        "commands": [
            {
                "name": "auth",
                "description": "Manage authentication tokens. Subcommands: login, logout, status.",
                "mutating": true,
                "subcommands": [
                    {
                        "name": "login",
                        "description": "Run the device-code flow and cache the resulting tokens.",
                        "mutating": true,
                        "args": [],
                        "output_fields": [
                            {"name": "username", "type": "string"},
                            {"name": "name", "type": "string"},
                            {"name": "tenant_id", "type": "string"}
                        ]
                    },
                    {
                        "name": "logout",
                        "description": "Delete cached tokens for the active profile.",
                        "mutating": true,
                        "args": [],
                        "output_fields": [
                            {"name": "removed", "type": "integer"}
                        ]
                    },
                    {
                        "name": "status",
                        "description": "Show cached account info, token expiry, and scopes. Works without network access.",
                        "mutating": false,
                        "args": [
                            {"name": "--limit", "type": "integer", "required": false, "default": 50, "description": "Maximum number of accounts to show."},
                            {"name": "--page", "type": "string", "required": false, "description": "Opaque pagination cursor from a previous response's `next` field."},
                            {"name": "--fields", "type": "string[]", "required": false, "description": "Comma-separated fields to include (e.g. username,expires_at)."}
                        ],
                        "output_fields": [
                            {"name": "total", "type": "integer"},
                            {"name": "next", "type": "string | null"},
                            {"name": "items", "type": "object[]"}
                        ]
                    }
                ]
            },
            {
                "name": "config",
                "description": "Inspect the resolved configuration. Subcommands: show, path.",
                "mutating": true,
                "subcommands": [
                    {
                        "name": "show",
                        "description": "Print the resolved config with secrets masked.",
                        "mutating": false,
                        "args": []
                    },
                    {
                        "name": "path",
                        "description": "Print the absolute path to the config file.",
                        "mutating": false,
                        "args": []
                    }
                ]
            },
            {
                "name": "init",
                "description": "Interactive setup: write a config profile and run the first device-code login.",
                "mutating": true,
                "args": [
                    {
                        "name": "--yes",
                        "type": "boolean",
                        "required": false,
                        "default": false,
                        "description": "Skip the final confirmation prompt and proceed with the provided values."
                    }
                ],
                "output_fields": [
                    {"name": "username", "type": "string"},
                    {"name": "name", "type": "string"},
                    {"name": "tenant_id", "type": "string"}
                ]
            },
            {
                "name": "sites",
                "description": "Discover and select SharePoint sites. Subcommands: list, use.",
                "mutating": true,
                "subcommands": [
                    {
                        "name": "list",
                        "description": "List followed sites (default) or search by query.",
                        "mutating": false,
                        "args": [
                            {"name": "--query", "type": "string", "required": false, "description": "Search query; omit to list followed sites."},
                            {"name": "--limit", "type": "integer", "required": false, "default": 50, "description": "Maximum number of items to return."},
                            {"name": "--offset", "type": "integer", "required": false, "default": 0, "description": "Number of items to skip (applied client-side)."},
                            {"name": "--all", "type": "boolean", "required": false, "default": false, "description": "Fetch all pages (ignores --limit)."},
                            {"name": "--page", "type": "string", "required": false, "description": "Opaque pagination cursor from a previous response's `next` field."},
                            {"name": "--fields", "type": "string[]", "required": false, "description": "Comma-separated list of fields to include in each item (e.g. id,name,url)."}
                        ],
                        "output_fields": [
                            {"name": "items", "type": "object[]"},
                            {"name": "total", "type": "integer"},
                            {"name": "next", "type": "string | null"},
                            {"name": "source", "type": "string"}
                        ]
                    },
                    {
                        "name": "use",
                        "description": "Set the default site for the active profile.",
                        "mutating": true,
                        "args": [
                            {"name": "site", "type": "string", "required": true, "description": "Site name or URL."},
                            {"name": "--yes", "type": "boolean", "required": false, "default": false, "description": "Skip confirmation prompt (required when stdin is not a TTY)."}
                        ],
                        "output_fields": [
                            {"name": "profile", "type": "string"},
                            {"name": "default_site", "type": "string"}
                        ]
                    }
                ]
            },
            {
                "name": "drives",
                "description": "List document libraries (drives) for a site. Subcommands: list.",
                "mutating": true,
                "subcommands": [
                    {
                        "name": "list",
                        "description": "List drives (document libraries) for a site.",
                        "mutating": false,
                        "args": [
                            {"name": "site", "type": "string", "required": true, "description": "Site reference: URL, spo://SiteName, or 'default'."},
                            {"name": "--limit", "type": "integer", "required": false, "default": 50, "description": "Maximum number of items to return."},
                            {"name": "--all", "type": "boolean", "required": false, "default": false, "description": "Return all drives (ignores --limit)."},
                            {"name": "--fields", "type": "string[]", "required": false, "description": "Comma-separated fields to include in each item (e.g. id,name,drive_type)."}
                        ],
                        "output_fields": [
                            {"name": "items", "type": "object[]"},
                            {"name": "total", "type": "integer"},
                            {"name": "next", "type": "string | null"}
                        ]
                    }
                ]
            },
            {
                "name": "files",
                "description": "Browse, search, and download files from SharePoint drives. Subcommands: ls, stat, download, find.",
                "mutating": true,
                "subcommands": [
                    {
                        "name": "ls",
                        "description": "List items in a folder.",
                        "mutating": false,
                        "args": [
                            {"name": "REF", "type": "string", "required": true, "description": "Reference in the form Site:Library/path or spo://Site/Library/path."},
                            {"name": "--recursive", "type": "boolean", "required": false, "default": false, "description": "List all items recursively. Cannot be combined with --limit/--all/--page."},
                            {"name": "--limit", "type": "integer", "required": false, "default": 200, "description": "Maximum number of items per response."},
                            {"name": "--all", "type": "boolean", "required": false, "default": false, "description": "Fetch all pages."},
                            {"name": "--page", "type": "string", "required": false, "description": "Opaque pagination cursor from a previous response's `next` field."},
                            {"name": "--fields", "type": "string[]", "required": false, "description": "Comma-separated fields to include in each item (e.g. name,size,kind)."}
                        ],
                        "output_fields": [
                            {"name": "items", "type": "object[]"},
                            {"name": "total", "type": "integer"},
                            {"name": "next", "type": "string | null"}
                        ]
                    },
                    {
                        "name": "stat",
                        "description": "Show metadata for a single file or folder.",
                        "mutating": false,
                        "args": [
                            {"name": "REF", "type": "string", "required": true, "description": "Reference to the item."}
                        ],
                        "output_fields": [
                            {"name": "id", "type": "string"},
                            {"name": "name", "type": "string"},
                            {"name": "kind", "type": "string"},
                            {"name": "size", "type": "integer"},
                            {"name": "modified", "type": "string | null"},
                            {"name": "download_url", "type": "string | null"},
                            {"name": "site", "type": "object"},
                            {"name": "drive", "type": "object"}
                        ]
                    },
                    {
                        "name": "download",
                        "description": "Download a file to a local path or stdout.",
                        "mutating": false,
                        "args": [
                            {"name": "REF", "type": "string", "required": true, "description": "Reference to the file."},
                            {"name": "--path", "type": "path", "required": false, "description": "Destination path. Use `-` to write to stdout."},
                            {"name": "--overwrite", "type": "boolean", "required": false, "default": false, "description": "Overwrite the file if it already exists."}
                        ],
                        "output_fields": [
                            {"name": "path", "type": "string"},
                            {"name": "bytes", "type": "integer"}
                        ]
                    },
                    {
                        "name": "find",
                        "description": "Search for files by query and/or name glob.",
                        "mutating": false,
                        "args": [
                            {"name": "REF", "type": "string", "required": true, "description": "Drive root reference (Site:Library or spo://Site/Library)."},
                            {"name": "--query", "type": "string", "required": false, "description": "Full-text search query."},
                            {"name": "--name", "type": "string", "required": false, "description": "Shell glob to filter results by file name."},
                            {"name": "--limit", "type": "integer", "required": false, "default": 200, "description": "Maximum number of results."},
                            {"name": "--all", "type": "boolean", "required": false, "default": false, "description": "Fetch all pages."},
                            {"name": "--page", "type": "string", "required": false, "description": "Opaque pagination cursor from a previous response's `next` field."},
                            {"name": "--fields", "type": "string[]", "required": false, "description": "Comma-separated fields to include in each item."}
                        ],
                        "output_fields": [
                            {"name": "items", "type": "object[]"},
                            {"name": "total", "type": "integer"},
                            {"name": "next", "type": "string | null"}
                        ]
                    }
                ]
            }
        ],
        "errors": [
            {
                "kind": "input",
                "exit_code": 2,
                "retryable": false,
                "description": "Bad user input, missing required argument, or invalid configuration."
            },
            {
                "kind": "auth",
                "exit_code": 3,
                "retryable": false,
                "description": "Authentication or token failure. Run `sharepoint auth login` to refresh."
            },
            {
                "kind": "read_only",
                "exit_code": 2,
                "retryable": false,
                "description": "Write operation blocked because SHAREPOINT_READ_ONLY=true or read_only=true in config."
            },
            {
                "kind": "confirmation_required",
                "exit_code": 2,
                "retryable": false,
                "description": "Destructive command refused without explicit --yes flag and no TTY available."
            },
            {
                "kind": "conflict",
                "exit_code": 7,
                "retryable": false,
                "description": "Resource already exists with an incompatible configuration."
            },
            {
                "kind": "not_found",
                "exit_code": 4,
                "retryable": false,
                "description": "Site, drive, or item not found."
            },
            {
                "kind": "api",
                "exit_code": 5,
                "retryable": false,
                "description": "Microsoft Graph API returned a non-2xx response."
            },
            {
                "kind": "rate_limit",
                "exit_code": 6,
                "retryable": true,
                "description": "Too many requests; Microsoft Graph is rate-limiting this client."
            },
            {
                "kind": "http",
                "exit_code": 1,
                "retryable": true,
                "description": "Underlying HTTP transport error (network failure, TLS, timeout)."
            },
            {
                "kind": "other",
                "exit_code": 1,
                "retryable": false,
                "description": "Unexpected internal error."
            }
        ]
    });

    println!(
        "{}",
        serde_json::to_string_pretty(&schema).expect("serialize schema")
    );
    Ok(())
}
