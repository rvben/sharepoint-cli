# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/).






## [0.0.6](https://github.com/rvben/sharepoint-cli/compare/v0.0.5...v0.0.6) - 2026-05-06

## [0.0.5](https://github.com/rvben/sharepoint-cli/compare/v0.0.4...v0.0.5) - 2026-05-06

### Added

- **graph**: accept HTTP-date in Retry-After header ([d0083da](https://github.com/rvben/sharepoint-cli/commit/d0083da415e8c9d9e38e8e0de5851f8719615ee7))
- **auth**: require client_id and surface a friendly error when missing ([268085f](https://github.com/rvben/sharepoint-cli/commit/268085f2c9af7ed0e9470a3b8928d2d047582975))

### Fixed

- **files**: use Option<usize> for --limit to detect explicit value ([ce4c14e](https://github.com/rvben/sharepoint-cli/commit/ce4c14e1b3cb2f24cf119d5d8f55382a99d577b9))
- **auth**: use SHAREPOINT_REFRESH_TOKEN to bootstrap auth ([a41e178](https://github.com/rvben/sharepoint-cli/commit/a41e178952a6e4ad94bd61c21f97ca354622060a))
- **auth**: apply redactor to OAuth2Error.error_description ([1230534](https://github.com/rvben/sharepoint-cli/commit/1230534cf6048f72955cf113089525804184ff0b))
- **auth**: release mutex across file I/O and network in access_token ([3f00c7a](https://github.com/rvben/sharepoint-cli/commit/3f00c7ab15a87ec2299a53a9e584620d8276eafd))
- **auth**: always emit device-code prompt under --quiet ([ad3ec75](https://github.com/rvben/sharepoint-cli/commit/ad3ec75a49a815b39e73cab80e1ff40dcd58407e))
- **auth**: retry transient errors on token endpoint ([1e9c5f5](https://github.com/rvben/sharepoint-cli/commit/1e9c5f58a847b347fe13d680289f0fcbb09b648a))
- **auth**: redact token-endpoint error bodies ([be11124](https://github.com/rvben/sharepoint-cli/commit/be11124c0d1bf9e039bf6b03145a26bf32c38a2e))
- **pagination**: cursor preserves mid-page progress ([88b9dab](https://github.com/rvben/sharepoint-cli/commit/88b9dabab8478bf2e3238c0afbb34999267565e0))
- **drives**: paginate drives list to fetch all pages ([7fd8a53](https://github.com/rvben/sharepoint-cli/commit/7fd8a53ec3816bdb22acc834eebc0a8e60866203))
- **files**: reject pagination flags when --recursive is set ([d6e7c2f](https://github.com/rvben/sharepoint-cli/commit/d6e7c2f3152e7f65cde51494e4e1f56496debd3e))
- **graph**: validate page-token host before attaching bearer ([ba954a6](https://github.com/rvben/sharepoint-cli/commit/ba954a6e78939bd47e1f3d8798e9fb501073db10))
- **graph**: percent-encode user query in search URL ([74b8b57](https://github.com/rvben/sharepoint-cli/commit/74b8b57829f9d3eacd8130f1b3a44e243528fc0b))
- **reference**: decode percent-escapes in colon-separated forms ([744736f](https://github.com/rvben/sharepoint-cli/commit/744736f0691e889dbbd4ce9cfc1529dc6478fd48))
- **reference**: stop double-decoding id= query values ([d2eb4e2](https://github.com/rvben/sharepoint-cli/commit/d2eb4e29730954ad7b31f5e7aca5fe62eb0da41c))
- **cli**: route clap parse errors through JSON-error-on-stdout contract ([a9e806e](https://github.com/rvben/sharepoint-cli/commit/a9e806e4bf4547e3852bddd49f786f3524e08d95))
- **auth**: canonicalize tenant to GUID after login ([20dae78](https://github.com/rvben/sharepoint-cli/commit/20dae7865c41d3e18a3f5b03253895aca9b78452))

## [0.0.4](https://github.com/rvben/sharepoint-cli/compare/v0.0.3...v0.0.4) - 2026-05-04

### Fixed

- **release**: rename PyPI distribution to sharepoint-cli-rs ([f51cd42](https://github.com/rvben/sharepoint-cli/commit/f51cd42520eea7f31e4dd1625b920fb828ba0e96))

## [0.0.3](https://github.com/rvben/sharepoint-cli/compare/v0.0.2...v0.0.3) - 2026-05-04

### Fixed

- **release**: use uv publish instead of twine for PyPI ([b262ca2](https://github.com/rvben/sharepoint-cli/commit/b262ca20f3effa0dba376d41d39ac73814e8d2b5))

## [0.0.2](https://github.com/rvben/sharepoint-cli/compare/v0.0.1...v0.0.2) - 2026-05-04

### Added

- **release**: add maturin/PyPI scaffolding and release workflow ([bae8c15](https://github.com/rvben/sharepoint-cli/commit/bae8c157df17866d2e222a325f8708d366ee809b))

## [0.0.1] - 2026-05-04

### Added

- **drives**: accept spo:// URI form in drives list site argument ([e94947d](https://github.com/rvben/sharepoint-cli/commit/e94947dde7d283ad21419a49ff99c4bb109b5289))
- implement config show/path with masked secrets ([c9ca3a7](https://github.com/rvben/sharepoint-cli/commit/c9ca3a70bf1a02723d6fdee1908da68d03272337))
- implement init with interactive setup and device-code login ([0776d2f](https://github.com/rvben/sharepoint-cli/commit/0776d2f413038ba68cead1db45ba54082623c166))
- **files**: implement ls/stat/download/find ([cae16c6](https://github.com/rvben/sharepoint-cli/commit/cae16c6ae67877b8530b613294db3585b173d498))
- **drives**: list libraries on a site ([d8254f7](https://github.com/rvben/sharepoint-cli/commit/d8254f74a4be0cdd3d9480b694f53b1299408693))
- **sites**: reject sites use in read-only mode ([eaddbab](https://github.com/rvben/sharepoint-cli/commit/eaddbabf9363285607b1ed940f5a335ea953e810))
- **sites**: implement list (followed/search) and use ([c2706e5](https://github.com/rvben/sharepoint-cli/commit/c2706e53cbe8504727ab278f2da798ba07ba6844))
- **auth**: implement login, logout, status commands ([8a4cdb1](https://github.com/rvben/sharepoint-cli/commit/8a4cdb19ced6e9c8f143798ef7e55b60365b6e4e))
- **cli**: add scaffold with clap derive subcommands and dispatcher ([9b9ac45](https://github.com/rvben/sharepoint-cli/commit/9b9ac45169f0b579fb887f38fbfc42dd8d5649c1))
- **graph**: add drive-scoped search with shell-glob matcher ([3966aa7](https://github.com/rvben/sharepoint-cli/commit/3966aa7e356fb804160c59617c46ee076e2bf7d9))
- **graph**: add streaming download ([873fc7a](https://github.com/rvben/sharepoint-cli/commit/873fc7a19100da101920bc3d6b46dce1e985f239))
- **graph**: add drive lookup, item listing, and canonical-shape mapper ([20eae65](https://github.com/rvben/sharepoint-cli/commit/20eae6512d4c17205dc330e705aa2a9bf2a632b1))
- **graph**: add site discovery with alias map and pagination ([b12b24c](https://github.com/rvben/sharepoint-cli/commit/b12b24c954af9afe27bfbba6cb4a21dace47182f))
- **graph**: add GraphClient with retry/backoff and paging ([3d7f22f](https://github.com/rvben/sharepoint-cli/commit/3d7f22f06a9933ceaf5afa13df1b2ebfa40f6707))
- **auth**: add AuthContext with auto-refresh and 60s margin ([1c8a536](https://github.com/rvben/sharepoint-cli/commit/1c8a536ef3500ecdf5ecdc12b2b19947993e07b4))
- **auth**: add device-code flow with polling state machine ([eb4ec4b](https://github.com/rvben/sharepoint-cli/commit/eb4ec4be0f253bdf3edc025c09530bf8e00f76c3))
- add token cache with atomic 0600 writes ([924ad00](https://github.com/rvben/sharepoint-cli/commit/924ad009ef0930327451327c59aefa12cf934c0d))
- add reference parser supporting all 5 input forms ([29f6452](https://github.com/rvben/sharepoint-cli/commit/29f64520c72c3f161e9590ff0193c2f209ba0d15))
- add config module with profiles, env overrides, and resolution order ([3965f93](https://github.com/rvben/sharepoint-cli/commit/3965f9337a98a6bc0a9cb51bf8ecf7a7f0e28cc6))
- add OutputConfig with JSON-error-on-stdout contract ([5c68729](https://github.com/rvben/sharepoint-cli/commit/5c687290bec2e33acda5c54f9be3407d73f47e0a))
- add CliError with structured exit codes ([39d719e](https://github.com/rvben/sharepoint-cli/commit/39d719ee8a6ebfde083622e257c113cc62877ffe))

### Fixed

- **files**: paginate find to match plan spec ([6c591fa](https://github.com/rvben/sharepoint-cli/commit/6c591fa9ed226e079c98860655c7e069ea7b1806))
- **files**: align ls/download/find with plan spec ([c8661a7](https://github.com/rvben/sharepoint-cli/commit/c8661a769aed14501dcb7c12f1c2179ab768bed6))
- **tests**: write auth-status fixture cache to binary's actual path ([501c45b](https://github.com/rvben/sharepoint-cli/commit/501c45b095e4db71006a8255c02f1ccf1723f196))
- **graph**: percent-encode drive paths and drop unwrap in canonical mapper ([f91f4df](https://github.com/rvben/sharepoint-cli/commit/f91f4df5680071ad1db5c592d0229b114778af69))
- **graph**: derive list source from decoded page token path ([d70527a](https://github.com/rvben/sharepoint-cli/commit/d70527a006b070a41f3fe999a9c43bc4fde477fb))
- **graph**: cap Retry-After at 60s and drop dead pow guard ([66d7c9c](https://github.com/rvben/sharepoint-cli/commit/66d7c9c2a353847eeec52e1da364356b70aeac65))
- **auth**: make refresh_token and Account.name Option<String> ([d30e5f7](https://github.com/rvben/sharepoint-cli/commit/d30e5f7d32e6b0496794176bdfc8ba5ad88fd1d5))
