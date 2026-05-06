# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/).




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
