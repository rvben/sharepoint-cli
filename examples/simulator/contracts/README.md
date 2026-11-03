# Independent response-shape corpus

`pnpcore.json` adapts six public PnP Core SDK server-response recordings from
commit `aba47bf58400a26ac7220997017be36d54d501fe`. The upstream project documents
that its offline fixtures contain saved responses from online tests:
[Writing test cases](https://pnp.github.io/pnpcore/contributing/writing%20tests.html).
`provenance.json` pins each original URL, SHA-256, and HTTP status. The upstream
MIT license is preserved in `LICENSE.pnpcore`.

These fixtures supply evidence independent of the generated simulator. They
cover identity properties, file metadata with a string `Length`, folder
metadata, two search pages containing typed and null cells, and an OData 404
error. `make test-simulator` runs these checks against the selected CLI binary
alongside the generated scenarios. CI runs the same checks through
`tests/simulator.rs`. No network access is needed to replay them.

## Adaptations and limits

This is a response-shape corpus, not a recording of this CLI talking to a live
tenant. The SDK's original operations and query selections differ from ours.
The fixtures adapt request paths, query parameters, names, and resource URLs to
the simulator's test data. The web and library-list setup responses are
synthetic; their collection envelope is not independent captured evidence.

Identity, file, and folder responses retain only the fields consumed by the
CLI. Identities, identifiers, filenames, and paths are replaced with dummy
values. File length, entity timestamps, the quoted ETag structure/version, and
the generic not-found error are retained from upstream. Search retains the
result counts, row counts, cell keys, `ValueType` annotations, value types, and
null cells. All non-null search cell values are replaced; unused envelope
properties are omitted. The source's total of 13,105 matches is retained, but
only two ten-row pages are present, so a third page or `--all` fails with a
missing-exchange error.

The original recordings' capture dates are not established here. Entity
timestamps are not capture dates. These fixtures do not establish current
service behavior, authentication compatibility, permission inheritance, search
ranking, actual KQL execution, downloads, or exact request compatibility.
Browser login and one complete live CLI smoke run still need a tenant.

## Reproduce the corpus

From the repository root, with Python 3 and internet access:

```sh
python3 scripts/import_pnpcore_contracts.py
```

The importer uses no tenant credentials. It downloads the pinned public files,
checks their SHA-256 values and expected statuses, projects and sanitizes their
bodies, and regenerates the fixtures and provenance. Raw recordings stay in
memory and response/request headers are excluded. Unknown search fields or
changed value types fail the import rather than copying unreviewed data.
Review the importer and provenance when changing the upstream revision.

To explore just these exchanges interactively:

```sh
cargo run --example simulator -- serve --fixtures examples/simulator/contracts/pnpcore.json
```

Use the dummy connection settings printed by the server and the isolated
profile instructions in [the testing guide](../../../docs/testing.md). Available
commands include `auth status`, `files stat` for `Documents/hello.txt`,
`Documents/Plans`, and `Documents/missing.txt`, and `files find` for the
Marketing Documents library with `--query '*'`. Only the exact requests in the
recording are available.
