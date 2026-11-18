# hexium-client

A native desktop client for [hexium.wtf](https://hexium.wtf), written in Rust with
[egui](https://github.com/emilk/egui). It talks to the site's own `/apisite/{namespace}{path}`
proxy, the same API the website uses.

Not affiliated with hexium.wtf or Roblox.

## What it does

- **Home**: a dashboard with your balances, what needs answering (messages, friend
  requests, trade offers), friends online and site events.
- **Browse**: games, the catalog (with item details; limiteds show a RAP and sales history chart
  and their resellers, cheapest first, with serials), groups.
- **Play**: the Play button in a game's details fetches a join ticket and starts your
  installed Hexium launcher through `hexium://`, the same way the site does.
  Below it, the game's running servers (players, ping, fps) each have a Join button.
  Friends in a server are outlined and listed; on the Friends screen, friends who are in a
  game have a Join friend button that finds their server and joins it.
- **You**: profile, avatar (body colours from the site's palette), inventory and
  collectibles, money and transaction history.
- **Social**: friends and friend requests, messages, trades with full trade details, and
  chat: your conversations and friends to start one with, with new messages and typing
  arriving live over the site's SignalR hub (`/chat`), falling back to polling when the
  hub is down.
  New chat messages that arrive while the client is in the background, or while you
  are on another screen or chat, show as Windows notifications (switchable in Settings).
  On first start the client adds a "hexium client" shortcut to your Start menu (per
  user, no admin) with its own AppUserModelID, so those notifications carry its name and
  icon; `hexium-client --register` does only that step.
- **One window**: launching the client again (from the Start menu, a shortcut, or a
  notification clicked in Action Center) brings the running one to the front instead
  of opening a second copy.
- **Open at sign-in** (Settings > Chat): starts the client hidden in the tray when you log into
  Windows (a per-user `Run` entry with `--minimized`), so chat notifications arrive
  without opening it by hand.
- **Tray icon**: closing the window keeps the client running in the notification area
  (switchable in Settings > Chat). Click the icon to open it; its menu has Open and
  Quit, and its tooltip shows how many chats are unread.
- **Activity notifications**: new inbound trades, friend requests and messages show as
  Windows notifications too (checked once a minute; switchable in Settings > Chat).
- **Writes**, each behind a confirmation dialog that says exactly what will change:
  follow / unfollow, favourite / unfavourite, set your status and description, vote on games, mark messages read or
  unread (or all at once), archive / unarchive messages, send / accept / decline friend requests, unfriend, send private messages
  and replies, send chat messages (starting the conversation if there is none yet), accept / decline / counter inbound trades and send new ones from a
  trade builder (the dialog lists both sides and their RAP totals, with optional Robux on either side), list a collectible for sale, change its price or take it off
  sale (with its RAP and the cheapest other listing, plus its price history chart with your
  price drawn across it).
- **Docked panel**: item, game, group and trade details, the trade builder, the message
  composer and the Sell window open in a panel on the right instead of floating windows,
  one at a time; ✕ or Esc closes it, and each kind remembers its width.
- **Values from [Heximons](https://heximons.lol)**: staff values, demand and trend for
  limiteds, read from its public catalog page every 15 minutes (cached; no cookie or
  account data is sent there). Shown in the catalog, inventory, trades and trade builder.
- **Trade analyzer**: each item's value, demand, trend and a small price-history line;
  a verdict for the whole trade by value and by RAP, with a warning for low-demand items.
- **Watchlist**: watch a limited at a target price and get a notification when its cheapest
  reseller reaches it (checked every 2 minutes). It only alerts; it never buys.
- **Value over time**: your value and RAP history from Heximons (every few minutes since
  it started tracking you), charted on the Inventory screen; the client's own daily record
  is the fallback.
- **Deals**: a Catalog view of limiteds whose cheapest listing is well below their value,
  biggest discount first, with the top rows checked against hexium.wtf's live prices.
- **Heximons on profiles**: value, RAP, rank and a value history chart for any player
  Heximons tracks, and a "View on Heximons" link on limiteds.
- **Trade results**: a notification when a trade you sent is accepted, declined, countered
  or expires.
- **Market** (from Heximons): staff value changes (notified when one hits your items),
  detected sales, trade ads (filter to ads wanting your items; open one straight in the
  trade builder) and the top-100 leaderboard.
- **Deal notifications**: optional, with a minimum discount, staff-valued-only and
  high-demand-only switches; each deal is announced once.
- **Charts**: 1D / 1W / 1M / All ranges with the change over the range.
- **Profit and loss**: what you paid for each collectible (from your purchases) against
  its value now, and a total.
- **Trade helpers** (its own screen in the side menu, under Social): pick what you want, build an offer by hand or
  with Suggest (closest value match), see everyone who owns it, open a trade with one of
  them, or mass-send the offer (after one confirmation listing them, skipping anyone you already
  trade with, with Stop). How many people (1–50, default 10) and the wait between trades
  (10–600 s, default 30) are set in Settings > Mass send.
  Or ask for a **value range**: the helper finds players whose items add up to it (up to four
  items each), lists one proposed trade per player for you to tick or untick, and sends the
  ticked ones the same way.
  Tick **Upgrade** (fewer, better items back) and/or **Downgrade** (more, smaller items back)
  to only get that kind of trade; each proposal is labelled.
  **Who to send to** filters apply to all of it: account age, how long they have held the
  item, online recently, skip people you already sent to (default 7 days), and the order
  (random by default, or newest / longest holders).
- **Your own values**: set a value on any limited; it replaces Heximons' (or adds one)
  everywhere values are used, and is labelled "Your value".
- **Trade stats** over your last 40 completed trades, and an Owners list on every limited.
- **Keyboard and display**: Ctrl+1–9 screens, Ctrl+F search, Ctrl+R refresh; text size and a
  compact layout in Settings.
- **Releases**: pushing a `v*` tag builds the Windows exe on GitHub and attaches it to a
  release.
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
endpoint (plus checks that a game's join ticket turns into a launch URL, without launching), plus a live handshake with the chat hub. Exit code 0 means everything passed.

## Layout

- `src/api/` — transport (`mod.rs`: cookies, CSRF retry like the site, no automatic
  retry of writes), the chat hub client (`hub.rs`), endpoint wrappers (`endpoints.rs`) and tolerant models (`models.rs`).
- `src/app.rs` — state, loading, and the write actions with their confirmation flow.
- `src/ui/` — the shell (theme, fonts, navigation, dialogs) and one file per screen.
- `src/check.rs` — the `--check` endpoint report.
