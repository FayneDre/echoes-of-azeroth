# Echoes of Azeroth

> Your own World of Warcraft server, installed in a few clicks.

Echoes of Azeroth is an open-source desktop launcher that installs, runs and
updates World of Warcraft server emulators and their modules, then launches
**your own** game client. No compiler, no database setup, no config files to edit.

**Status: early development (phase 0).** Nothing is installable yet.

## First target

- [AzerothCore](https://www.azerothcore.org) (WotLK 3.3.5a), Playerbot fork
- Modules: [Playerbots](https://github.com/mod-playerbots/mod-playerbots) (always installed),
  [Auction House Bot](https://github.com/azerothcore/mod-ah-bot) and
  [Individual Progression](https://github.com/ZhengPeiRu21/mod-individual-progression)
  (optional, add or remove at any time)
- Solo and LAN play
- English and French

More emulators, clients and modules will follow; the architecture is built for it.

## Development

Requirements (Windows): [Rust](https://rustup.rs) (MSVC toolchain + Visual Studio
Build Tools "Desktop development with C++"), Node.js 22+, WebView2 (bundled with Windows 11).

```sh
npm install
npm run dev                   # launcher in development mode
npm run check                 # UI type-check + catalog schema validation
cargo test --workspace        # Rust tests (including the real catalog)
cargo run -p eoa-cli -- catalog validate
```

`npm install` also enables the repository git hooks. See
[`CONTRIBUTING.md`](CONTRIBUTING.md) for the full workflow and rules, and
[`docs/architecture.md`](docs/architecture.md), [`docs/roadmap.md`](docs/roadmap.md)
and [`docs/decisions/`](docs/decisions/) for the design.

## License

Echoes of Azeroth is free software under the
[GNU General Public License v3.0 or later](LICENSE).
Security issues: see [`SECURITY.md`](SECURITY.md).

## Legal

World of Warcraft is a trademark of Blizzard Entertainment. This project is not
affiliated with Blizzard. The game client is never distributed: you must provide
your own. Server emulators and modules are distributed under their own licenses
(AzerothCore: GPL-2.0), built from the exact source commits listed in `catalog/`.
