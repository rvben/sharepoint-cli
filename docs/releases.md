# Releases

Vership is the release control plane for `sharepoint-cli`. GitHub Actions is the
execution plane for cross-platform builds, publication, provenance, and
downstream packaging.

## Create a release

Start from a clean, up-to-date `main` branch and run:

```sh
vership preflight
vership bump patch --prepare # or minor/major; creates the release commit locally
# After approval to push and publish:
vership release
# Wait for the tag's Release workflow to finish, then:
vership verify --targets tag,release,crates,pypi
```

Before preparing, inspect `vership status`, `vership config show`, and the
installed command's `--help`. Preparation runs `make check`, updates the package
version and changelog, and creates the Conventional Commit release commit.
Review and test that commit, then use `vership release` to tag and push the
prepared version without bumping again. The tag starts the release workflow.
Publishing requires explicit approval; an instruction to release a named version
provides that authorization.

Wait for CI on Linux, macOS, and Windows and the publication workflow. Verify
all four targets, download a published binary, check its version and checksum,
and run `cargo run --example simulator -- smoke --cli /path/to/sharepoint`.
Finally fetch and fast-forward the primary checkout to the remote default branch,
confirming that the release tag and remote branch reference the tested commit.

Cargo and Python package versions are kept synchronized as one release unit.

## Failure policy

If a release fails before any release, artifact, package, checksum, or
attestation becomes public, delete and recreate the brief tag and retry the
same version. Once anything was published, preserve the tag and issue a patch
release for release-content corrections. Registry and downstream-packaging
jobs are separate recovery domains; retry only the failed job when possible.

## Dry runs

The Release workflow can be dispatched from `main` with `dry_run` enabled. It
uses the package version unless an explicit `version` is supplied, builds the
real artifacts, and exercises registry validation without publishing packages,
creating a GitHub release, generating attestations, or updating downstream
packaging.
