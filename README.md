# hexium-client

A native desktop client for [hexium.wtf](https://hexium.wtf), written in Rust with
[egui](https://github.com/emilk/egui). It talks to the site's own `/apisite/{namespace}{path}`
proxy, the same API the website uses.

Not affiliated with hexium.wtf or Roblox.

## What it does

- **Home**: a dashboard with your balances, what needs answering (messages, friend
  requests, trade offers), friends online and site events.
- **Browse**: games, the catalog (with item details), groups.
- **You**: profile, avatar (body colours from the site's palette), inventory and
  collectibles, money and transaction history.
- **Social**: friends and friend requests, messages, trades with full trade details.
- **Writes**, each behind a confirmation dialog that says exactly what will change:
  follow / unfollow, favourite / unfavourite, set your status and description, vote on games, mark messages read or
  unread (or all at once), archive / unarchive messages, send / accept / decline friend requests, unfriend, send private messages
  and replies, accept / decline / counter inbound trades and send new ones from a
  trade builder (the dialog lists both sides and their RAP totals), list a collectible for sale, change its price or take it off
  sale (with its RAP and the cheapest other listing).
- **Make it yours**: pick an accent colour and background in Settings.

Purchases are deliberately not supported: the site's buy flow includes an anti-bot
proof that a native client would have to fake.

## Build and run

```
cargo build --release
target/release/hexium-client.exe
```

Sign in by pasting your `.ROBLOSECURITY` cookie value for hexium.wtf. It is stored in
`%APPDATA%\hexium-client\config.json` (or read from the `HEXIUM_COOKIE` environment
variable) and is only ever sent to hexium.wtf. Never share it: it is your account.

## Endpoint check

```
hexium-client --check
```

Calls every read endpoint once with your session and reports what parsed, one line per
endpoint. Exit code 0 means everything passed.

## Layout

- `src/api/` — transport (`mod.rs`: cookies, CSRF retry like the site, no automatic
  retry of writes), endpoint wrappers (`endpoints.rs`) and tolerant models (`models.rs`).
- `src/app.rs` — state, loading, and the write actions with their confirmation flow.
- `src/ui/` — the shell (theme, fonts, navigation, dialogs) and one file per screen.
- `src/check.rs` — the `--check` endpoint report.
