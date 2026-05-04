# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/).



## [0.0.3](https://github.com/rvben/sharepoint-cli/compare/v0.0.2...v0.0.3) - 2026-05-04

### Fixed

- **release**: use uv publish instead of twine for PyPI ([b679f57](https://github.com/rvben/sharepoint-cli/commit/b679f57d44ce793ef766f7e8b9472a055225682a))

## [0.0.2](https://github.com/rvben/sharepoint-cli/compare/v0.0.1...v0.0.2) - 2026-05-04

### Added

- **release**: add maturin/PyPI scaffolding and release workflow ([020f4fa](https://github.com/rvben/sharepoint-cli/commit/020f4fa1861056ad76b7a118610b7ca4fde71eec))

## [0.0.1] - 2026-05-04

### Added

- **drives**: accept spo:// URI form in drives list site argument ([20fcf2d](https://github.com/rvben/sharepoint-cli/commit/20fcf2d80060e9f6aab2ecd0cdb7f110bded53a1))
- implement config show/path with masked secrets ([fe60fe1](https://github.com/rvben/sharepoint-cli/commit/fe60fe1a8815389db0649538e91cf4c307039a34))
- implement init with interactive setup and device-code login ([209648d](https://github.com/rvben/sharepoint-cli/commit/209648d20096fb034b6870f6c43f995eac19247e))
- **files**: implement ls/stat/download/find ([fb5bd4a](https://github.com/rvben/sharepoint-cli/commit/fb5bd4ab61b7f0ae7b6d7c988060e44339e9ddad))
- **drives**: list libraries on a site ([9121d98](https://github.com/rvben/sharepoint-cli/commit/9121d98824f4d1e942cd3f57e16bd18e715a636f))
- **sites**: reject sites use in read-only mode ([899013c](https://github.com/rvben/sharepoint-cli/commit/899013cbeb712e9bfc75555c51e2dc4e38e1dc80))
- **sites**: implement list (followed/search) and use ([3283798](https://github.com/rvben/sharepoint-cli/commit/32837985772279ffcfe86eb421f22117fd7d438e))
- **auth**: implement login, logout, status commands ([ebf3e15](https://github.com/rvben/sharepoint-cli/commit/ebf3e154caa361c169ff1e002c5a6f5376bdd385))
- **cli**: add scaffold with clap derive subcommands and dispatcher ([1ef986c](https://github.com/rvben/sharepoint-cli/commit/1ef986cff880c8a6c60ad405e7a8e806423d33d4))
- **graph**: add drive-scoped search with shell-glob matcher ([93518a0](https://github.com/rvben/sharepoint-cli/commit/93518a0f7fb3e9fbc6afb1db54fbca59c09ac252))
- **graph**: add streaming download ([fa1c245](https://github.com/rvben/sharepoint-cli/commit/fa1c245348a5ba180fb2d5ff9141099e898e7a3a))
- **graph**: add drive lookup, item listing, and canonical-shape mapper ([0fd78fa](https://github.com/rvben/sharepoint-cli/commit/0fd78fac8bbc668adfde241d9b3a58553d3892ef))
- **graph**: add site discovery with alias map and pagination ([87524ac](https://github.com/rvben/sharepoint-cli/commit/87524ac05ac357d61cdd66970724f13d6c7840e9))
- **graph**: add GraphClient with retry/backoff and paging ([264cf7f](https://github.com/rvben/sharepoint-cli/commit/264cf7f0298613959482127ba400f1cfa27f5106))
- **auth**: add AuthContext with auto-refresh and 60s margin ([c5aa3f7](https://github.com/rvben/sharepoint-cli/commit/c5aa3f7b99fe30d19bada6a9905eb32a7b23bfe9))
- **auth**: add device-code flow with polling state machine ([ce0fec5](https://github.com/rvben/sharepoint-cli/commit/ce0fec5b5d585d5c9017ffd1ef5b9f8b7c43cbf7))
- add token cache with atomic 0600 writes ([3253223](https://github.com/rvben/sharepoint-cli/commit/32532235b2b603adb2e3f07a25eb34bce0f4e8b8))
- add reference parser supporting all 5 input forms ([e339721](https://github.com/rvben/sharepoint-cli/commit/e339721b2981bf78074a459b7c688b39b2aabbad))
- add config module with profiles, env overrides, and resolution order ([5a57136](https://github.com/rvben/sharepoint-cli/commit/5a57136abfdcb6e2118c4e686f149e46993049e3))
- add OutputConfig with JSON-error-on-stdout contract ([d17bb32](https://github.com/rvben/sharepoint-cli/commit/d17bb32845ea6812e7904d1dc9f2eddac045c146))
- add CliError with structured exit codes ([f3e96d2](https://github.com/rvben/sharepoint-cli/commit/f3e96d244db9e4cf4c06cb932de7f9942fc448f9))

### Fixed

- **files**: paginate find to match plan spec ([78ddba2](https://github.com/rvben/sharepoint-cli/commit/78ddba28ca2abee25a4273ff7d3f6fcd372fdfe3))
- **files**: align ls/download/find with plan spec ([80e6408](https://github.com/rvben/sharepoint-cli/commit/80e6408b32f77ddba6ae1a0e9b8bdf54c7d45b52))
- **tests**: write auth-status fixture cache to binary's actual path ([1bf6f18](https://github.com/rvben/sharepoint-cli/commit/1bf6f1885b56c22de2de00d7bbe7b0002c689c3e))
- **graph**: percent-encode drive paths and drop unwrap in canonical mapper ([f09d593](https://github.com/rvben/sharepoint-cli/commit/f09d593ade6b57b69ea7629a227006b0ae339396))
- **graph**: derive list source from decoded page token path ([bbb5838](https://github.com/rvben/sharepoint-cli/commit/bbb5838b8259f66dc75bdfae5e7ff29cfbb1b273))
- **graph**: cap Retry-After at 60s and drop dead pow guard ([10c509a](https://github.com/rvben/sharepoint-cli/commit/10c509a81b14a37c9a5c9cebaea5b7fe9a0df24a))
- **auth**: make refresh_token and Account.name Option<String> ([1c4cabe](https://github.com/rvben/sharepoint-cli/commit/1c4cabe48d746779f7e76cb00f89da7587bbc71a))
