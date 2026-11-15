//! `--check`: hit every read endpoint once and report what actually parsed.
//!
//! Reads the session cookie from `HEXIUM_COOKIE` (falling back to the cookie the
//! GUI stored), walks the documented read surface sequentially, and prints one
//! line per endpoint: verdict, path, latency and what came back. Exit codes:
//! `0` everything passed, `1` at least one failure, `2` no cookie available.
//!
//! The cookie itself is never printed, logged or summarised.

use std::time::Instant;

use serde_json::Value;

use crate::api::{endpoints, models, ApiClient, ApiError};
use crate::config::Config;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Pass,
    Warn,
    Fail,
}

struct Row {
    name: &'static str,
    detail: String,
    verdict: Verdict,
    ms: u128,
}

/// Gives the assertion closures a concrete signature so their parameters infer.
fn judge<F>(assert: F, status: u16, value: &Value) -> (Verdict, String)
where
    F: FnOnce(u16, &Value) -> (Verdict, String),
{
    assert(status, value)
}

/// One GET check: transport errors are failures, the closure decides the rest.
macro_rules! check {
    ($rows:ident, $client:expr, $name:literal, $path:expr, $query:expr, $assert:expr) => {{
        let target = $path.to_string();
        let started = Instant::now();
        let (verdict, detail) = match $client.get_query_with_status(&target, $query).await {
            Ok((status, value)) => judge($assert, status, &value),
            Err(error) => (Verdict::Fail, error.to_string()),
        };
        $rows.push(Row {
            name: $name,
            detail,
            verdict,
            ms: started.elapsed().as_millis(),
        });
    }};
}

/// One POST check (CSRF path included).
macro_rules! check_post {
    ($rows:ident, $client:expr, $name:literal, $path:expr, $body:expr, $assert:expr) => {{
        let target = $path.to_string();
        let started = Instant::now();
        let (verdict, detail) = match $client.post_with_status(&target, $body).await {
            Ok((status, value)) => judge($assert, status, &value),
            Err(error) => (Verdict::Fail, error.to_string()),
        };
        $rows.push(Row {
            name: $name,
            detail,
            verdict,
            ms: started.elapsed().as_millis(),
        });
    }};
}

/// A check for endpoints the client does not use in the UI: a 4xx here means the
/// query needs parameters we cannot discover, not that parsing broke.
macro_rules! check_soft {
    ($rows:ident, $client:expr, $name:literal, $path:expr, $query:expr, $assert:expr) => {{
        let target = $path.to_string();
        let started = Instant::now();
        let (verdict, detail) = match $client.get_query_with_status(&target, $query).await {
            Ok((status, value)) => judge($assert, status, &value),
            Err(ApiError::Rejected(status, body)) => (Verdict::Warn, format!("HTTP {status} · {body}")),
            Err(ApiError::NotFound) => {
                (Verdict::Warn, "HTTP 404 · nothing for this id".to_string())
            }
            Err(error) => (Verdict::Fail, error.to_string()),
        };
        $rows.push(Row {
            name: $name,
            detail,
            verdict,
            ms: started.elapsed().as_millis(),
        });
    }};
}

pub fn run() -> i32 {
    let config = crate::config::load();
    let from_env = std::env::var("HEXIUM_COOKIE")
        .ok()
        .map(|value| Config::normalize_cookie(&value))
        .filter(|value| !value.is_empty());
    let source = if from_env.is_some() {
        "HEXIUM_COOKIE"
    } else {
        "config file"
    };
    let Some(cookie) = from_env.or_else(|| config.session_cookie.clone()) else {
        eprintln!("No session cookie found.");
        eprintln!(
            "Set HEXIUM_COOKIE to a .ROBLOSECURITY value and run again, or start the client once and sign in."
        );
        return 2;
    };

    let client = ApiClient::new(&config.origin, Some(&cookie));
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("could not start tokio: {error}");
            return 2;
        }
    };

    let rows = runtime.block_on(run_all(&client));

    println!("hexium client · read endpoint check");
    println!("endpoint   {}", client.origin());
    println!("cookie     {source}");
    println!("user-agent {}\n", crate::api::USER_AGENT);

    for row in &rows {
        let tag = match row.verdict {
            Verdict::Pass => "PASS",
            Verdict::Warn => "WARN",
            Verdict::Fail => "FAIL",
        };
        println!("{tag}  {:<46} {:>5} ms  {}", row.name, row.ms, row.detail);
    }

    let passed = rows.iter().filter(|row| row.verdict == Verdict::Pass).count();
    let warned = rows.iter().filter(|row| row.verdict == Verdict::Warn).count();
    let failed = rows.iter().filter(|row| row.verdict == Verdict::Fail).count();

    println!(
        "\n{passed} passed · {warned} warnings · {failed} failed · {} checks",
        rows.len()
    );

    if failed > 0 {
        println!("\nFailures:");
        for row in rows.iter().filter(|row| row.verdict == Verdict::Fail) {
            println!("  {:<46} {}", row.name, row.detail);
        }
        return 1;
    }
    0
}

async fn run_all(client: &ApiClient) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();

    // -- identity ----------------------------------------------------------
    let me = {
        let started = Instant::now();
        let (verdict, detail, id) =
            match client.get(&p("users", "/v1/users/authenticated")).await {
                Ok(value) => {
                    let user = models::AuthUser::from_value(&value);
                    if user.id == 0 {
                        (
                            Verdict::Fail,
                            format!("no id in the response; keys: {}", keys(&value)),
                            0,
                        )
                    } else {
                        (
                            Verdict::Pass,
                            format!("id={} @{} ({})", user.id, user.name, user.display_name),
                            user.id,
                        )
                    }
                }
                Err(error) => (Verdict::Fail, error.to_string(), 0),
            };
        rows.push(Row {
            name: "users/v1/users/authenticated",
            detail,
            verdict,
            ms: started.elapsed().as_millis(),
        });
        id
    };

    if me == 0 {
        rows.push(Row {
            name: "-- stopped --",
            detail: "the remaining checks all need an authenticated user id".to_string(),
            verdict: Verdict::Fail,
            ms: 0,
        });
        return rows;
    }

    // Everything below is keyed off ids discovered as we go.
    let mut friend = 0u64;
    let mut group = 0u64;
    let mut universe = 0u64;
    let mut asset = 0u64;

    // -- users -------------------------------------------------------------
    check!(
        rows, client,
        "users/v1/users/{id}",
        &p("users", &format!("/v1/users/{me}")),
        &[],
        |status, value| {
            let user = models::UserProfile::from_value(value);
            if user.id == 0 {
                fail(status, value)
            } else {
                ok(format!(
                    "HTTP {status} · @{} · joined {}",
                    user.name,
                    first(&user.created)
                ))
            }
        }
    );

    check!(
        rows, client,
        "users/v1/users/{id}/status",
        &p("users", &format!("/v1/users/{me}/status")),
        &[],
        |status, value| {
            let text = models::str_at(value, "status");
            if text.is_empty() {
                warn(format!(
                    "HTTP {status} · status is empty (the account may simply have none)"
                ))
            } else {
                ok(format!("HTTP {status} · {} chars", text.chars().count()))
            }
        }
    );

    check!(
        rows, client,
        "users/v1/users/{id}/username-history",
        &p("users", &format!("/v1/users/{me}/username-history")),
        &[("limit", "10".to_string())],
        |status, value| {
            let names = models::first_array(value, &["data"]);
            ok(format!("HTTP {status} · {} name(s)", names.len()))
        }
    );

    check!(
        rows, client,
        "users/v1/users/star-creator-status",
        &p("users", "/v1/users/star-creator-status"),
        &[],
        |status, value| object_like(status, value, "isStarCreator")
    );

    // -- economy -----------------------------------------------------------
    check!(
        rows, client,
        "economy/v1/users/{id}/currency",
        &p("economy", &format!("/v1/users/{me}/currency")),
        &[],
        |status, value| {
            let currency = models::Currency::from_value(value);
            if currency.robux == 0 && currency.tix == 0 {
                warn(format!(
                    "HTTP {status} · robux=0 tix=0 · keys: {}",
                    keys(value)
                ))
            } else {
                ok(format!(
                    "HTTP {status} · robux={} tix={}",
                    currency.robux, currency.tix
                ))
            }
        }
    );

    // One check per type the site's Money page offers. `adrevenue` is in the site's
    // own picker but the server rejects it, so it lands as a warning.
    check_soft!(
        rows, client,
        "economy/…/transactions · purchase",
        &p("economy", &format!("/v2/users/{me}/transactions")),
        &[("cursor", String::new()), ("transactionType", "purchase".to_string())],
        |status, value| array_like(status, value, &["data"])
    );
    check_soft!(
        rows, client,
        "economy/…/transactions · sale",
        &p("economy", &format!("/v2/users/{me}/transactions")),
        &[("cursor", String::new()), ("transactionType", "sale".to_string())],
        |status, value| array_like(status, value, &["data"])
    );
    check_soft!(
        rows, client,
        "economy/…/transactions · commision",
        &p("economy", &format!("/v2/users/{me}/transactions")),
        &[("cursor", String::new()), ("transactionType", "commision".to_string())],
        |status, value| array_like(status, value, &["data"])
    );
    check_soft!(
        rows, client,
        "economy/…/transactions · adrevenue",
        &p("economy", &format!("/v2/users/{me}/transactions")),
        &[("cursor", String::new()), ("transactionType", "adrevenue".to_string())],
        |status, value| array_like(status, value, &["data"])
    );
    check_soft!(
        rows, client,
        "economy/…/transactions · group-payout",
        &p("economy", &format!("/v2/users/{me}/transactions")),
        &[("cursor", String::new()), ("transactionType", "group-payout".to_string())],
        |status, value| array_like(status, value, &["data"])
    );

    // -- friends -----------------------------------------------------------
    check!(
        rows, client,
        "friends/v1/users/{id}/friends",
        &p("friends", &format!("/v1/users/{me}/friends")),
        &[],
        |status, value| {
            let friends = models::first_array(value, &["data", "friends"]);
            if let Some(entry) = friends.first() {
                friend = models::num_at(entry, "id");
            }
            ok(format!("HTTP {status} · {} friend(s)", friends.len()))
        }
    );

    check!(
        rows, client,
        "friends/v1/user/friend-requests/count",
        &p("friends", "/v1/user/friend-requests/count"),
        &[],
        |status, value| count_like(status, value)
    );

    check!(
        rows, client,
        "friends/v1/users/{id}/followers/count",
        &p("friends", &format!("/v1/users/{me}/followers/count")),
        &[],
        |status, value| count_like(status, value)
    );

    check!(
        rows, client,
        "friends/v1/users/{id}/followings/count",
        &p("friends", &format!("/v1/users/{me}/followings/count")),
        &[],
        |status, value| count_like(status, value)
    );

    check!(
        rows, client,
        "friends/v1/my/friends/requests",
        &p("friends", "/v1/my/friends/requests"),
        &[("limit", "10".to_string())],
        |status, value| array_like(status, value, &["data"])
    );

    if friend == 0 {
        rows.push(skipped(
            "friends/v1/users/{id}/friends/statuses",
            "no friend id to probe",
        ));
        rows.push(skipped(
            "friends/v1/user/following-exists",
            "no friend id to probe",
        ));
    } else {
        check!(
            rows, client,
            "friends/v1/users/{id}/friends/statuses",
            &p("friends", &format!("/v1/users/{me}/friends/statuses")),
            &[("userIds", friend.to_string())],
            |status, value| array_like(status, value, &["data"])
        );

        check_post!(
            rows, client,
            "friends/v1/user/following-exists",
            &p("friends", "/v1/user/following-exists"),
            Some(serde_json::json!({ "targetUserIds": [friend] })),
            |status, value| {
                if value.get("followings").is_some() {
                    ok(format!(
                        "HTTP {status} · {} entries",
                        models::first_array(value, &["followings"]).len()
                    ))
                } else {
                    fail(status, value)
                }
            }
        );
    }

    check!(
        rows, client,
        "privatemessages/v1/messages/unread/count",
        &p("privatemessages", "/v1/messages/unread/count"),
        &[],
        |status, value| count_like(status, value)
    );

    check_soft!(
        rows, client,
        "privatemessages/…/messages · inbox",
        &p("privatemessages", "/v1/messages"),
        &[
            ("messageTab", "inbox".to_string()),
            ("pageSize", "20".to_string()),
            ("pageNumber", "0".to_string()),
        ],
        |status, value| array_like(status, value, &["collection"])
    );
    check_soft!(
        rows, client,
        "privatemessages/…/messages · sent",
        &p("privatemessages", "/v1/messages"),
        &[
            ("messageTab", "sent".to_string()),
            ("pageSize", "20".to_string()),
            ("pageNumber", "0".to_string()),
        ],
        |status, value| array_like(status, value, &["collection"])
    );
    check_soft!(
        rows, client,
        "privatemessages/…/messages · archive",
        &p("privatemessages", "/v1/messages"),
        &[
            ("messageTab", "archive".to_string()),
            ("pageSize", "20".to_string()),
            ("pageNumber", "0".to_string()),
        ],
        |status, value| array_like(status, value, &["collection"])
    );
    check_soft!(
        rows, client,
        "privatemessages/v1/announcements",
        &p("privatemessages", "/v1/announcements"),
        &[],
        |status, value| array_like(status, value, &["collection"])
    );

    check!(
        rows, client,
        "trades/v1/trades/inbound/count",
        &p("trades", "/v1/trades/inbound/count"),
        &[],
        |status, value| count_like(status, value)
    );

    check_soft!(
        rows, client,
        "trades/v1/trades/inbound",
        &p("trades", "/v1/trades/inbound"),
        &[("cursor", String::new())],
        |status, value| array_like(status, value, &["data"])
    );
    check_soft!(
        rows, client,
        "trades/v1/trades/outbound",
        &p("trades", "/v1/trades/outbound"),
        &[("cursor", String::new())],
        |status, value| array_like(status, value, &["data"])
    );
    check_soft!(
        rows, client,
        "trades/v1/trades/completed",
        &p("trades", "/v1/trades/completed"),
        &[("cursor", String::new())],
        |status, value| array_like(status, value, &["data"])
    );
    check_soft!(
        rows, client,
        "trades/v1/trades/inactive",
        &p("trades", "/v1/trades/inactive"),
        &[("cursor", String::new())],
        |status, value| array_like(status, value, &["data"])
    );

    // Details need a real trade id, so borrow one from the first non-empty list.
    let mut trade_id = 0;
    for tab in ["completed", "inactive", "outbound", "inbound"] {
        if let Ok(page) = endpoints::trades(client, tab, None).await {
            if let Some(trade) = page.items.first() {
                trade_id = trade.id;
                break;
            }
        }
    }
    if trade_id == 0 {
        rows.push(skipped("trades/v1/trades/{id}", "this account has no trades"));
    } else {
        check!(
            rows, client,
            "trades/v1/trades/{id}",
            &p("trades", &format!("/v1/trades/{trade_id}")),
            &[],
            |status, value| object_like(status, value, "offers")
        );
    }


    // -- inventory & avatar ------------------------------------------------
    check!(
        rows, client,
        "inventory/v1/users/{id}/owned-asset-ids",
        &p("inventory", &format!("/v1/users/{me}/owned-asset-ids")),
        &[],
        |status, value| {
            let ids = models::first_array(value, &["assetIds", "data"]);
            if let Some(entry) = ids.first() {
                asset = entry
                    .as_u64()
                    .or_else(|| models::opt_num_at(entry, "assetId"))
                    .unwrap_or(0);
            }
            ok(format!("HTTP {status} · {} asset id(s)", ids.len()))
        }
    );

    check!(
        rows, client,
        "inventory/v1/users/{id}/assets/collectibles",
        &p("inventory", &format!("/v1/users/{me}/assets/collectibles")),
        &[("limit", "50".to_string())],
        |status, value| array_like(status, value, &["data", "items"])
    );

    check!(
        rows, client,
        "avatar/v1/users/{id}/avatar",
        &p("avatar", &format!("/v1/users/{me}/avatar")),
        &[],
        |status, value| {
            let avatar = models::AvatarData::from_value(value);
            if avatar.assets.is_empty() && avatar.scales.is_empty() {
                warn(format!(
                    "HTTP {status} · no assets and no scales · keys: {}",
                    keys(value)
                ))
            } else {
                ok(format!(
                    "HTTP {status} · {} worn asset(s) · {} scale(s)",
                    avatar.assets.len(),
                    avatar.scales.len()
                ))
            }
        }
    );

    // -- groups ------------------------------------------------------------
    check!(
        rows, client,
        "groups/v2/users/{id}/groups/roles",
        &p("groups", &format!("/v2/users/{me}/groups/roles")),
        &[],
        |status, value| {
            let groups = models::first_array(value, &["data"]);
            if let Some(entry) = groups.first() {
                let parsed = models::UserGroup::from_value(entry);
                group = parsed.id;
                if parsed.name.is_empty() {
                    return warn(format!(
                        "HTTP {status} · {} entries but no group name · keys: {}",
                        groups.len(),
                        keys(entry)
                    ));
                }
            }
            ok(format!("HTTP {status} · {} group(s)", groups.len()))
        }
    );

    check!(
        rows, client,
        "groups/v1/users/{id}/groups/roles",
        &p("groups", &format!("/v1/users/{me}/groups/roles")),
        &[],
        |status, value| array_like(status, value, &["data"])
    );

    check!(
        rows, client,
        "groups/v1/users/{id}/groups/primary/role",
        &p("groups", &format!("/v1/users/{me}/groups/primary/role")),
        &[],
        |status, value| {
            if value.is_null() {
                ok(format!("HTTP {status} · empty body (204-style answer)"))
            } else {
                ok(format!("HTTP {status} · {}", summarise(value)))
            }
        }
    );

    if group == 0 {
        rows.push(skipped(
            "groups/v1/groups/{id}",
            "this account is in no groups",
        ));
        rows.push(skipped(
            "groups/v1/groups/{id}/roles",
            "this account is in no groups",
        ));
        rows.push(skipped(
            "groups/v2/groups/{id}/wall/posts",
            "this account is in no groups",
        ));
        rows.push(skipped(
            "thumbnails/v1/groups/icons",
            "this account is in no groups",
        ));
    } else {
        check!(
            rows, client,
            "groups/v1/groups/{id}",
            &p("groups", &format!("/v1/groups/{group}")),
            &[],
            |status, value| {
                let info = models::GroupInfo::from_value(value);
                if info.name.is_empty() {
                    fail(status, value)
                } else {
                    ok(format!(
                        "HTTP {status} · {} · {} members",
                        info.name, info.member_count
                    ))
                }
            }
        );

        check!(
            rows, client,
            "groups/v1/groups/{id}/roles",
            &p("groups", &format!("/v1/groups/{group}/roles")),
            &[],
            |status, value| array_like(status, value, &["roles", "data"])
        );

        check!(
            rows, client,
            "groups/v2/groups/{id}/wall/posts",
            &p("groups", &format!("/v2/groups/{group}/wall/posts")),
            &[("limit", "10".to_string()), ("sortOrder", "Desc".to_string())],
            |status, value| array_like(status, value, &["data"])
        );

        check!(
            rows, client,
            "thumbnails/v1/groups/icons",
            &p("thumbnails", "/v1/groups/icons"),
            &[
                ("groupIds", group.to_string()),
                ("size", "150x150".to_string()),
                ("format", "Png".to_string()),
            ],
            |status, value| thumbnail_like(status, value)
        );
    }

    // -- games -------------------------------------------------------------
    check!(
        rows, client,
        "games/v1/games/sorts",
        &p("games", "/v1/games/sorts"),
        &[("gameSortsContext", "GamesDefaultSorts".to_string())],
        |status, value| {
            let sorts = models::first_array(value, &["games", "sorts", "data"]);
            if sorts.is_empty() {
                warn(format!("HTTP {status} · 0 sorts · keys: {}", keys(value)))
            } else {
                ok(format!("HTTP {status} · {} sort(s)", sorts.len()))
            }
        }
    );

    check!(
        rows, client,
        "games/v1/games/list",
        &p("games", "/v1/games/list"),
        &[("maxRows", "10".to_string())],
        |status, value| {
            let games = models::first_array(value, &["games", "data"]);
            if let Some(entry) = games.first() {
                let parsed = models::Game::from_value(entry);
                if parsed.id != 0 {
                    universe = parsed.id;
                }
                if parsed.name.is_empty() {
                    return warn(format!(
                        "HTTP {status} · {} entries but no name · keys: {}",
                        games.len(),
                        keys(entry)
                    ));
                }
            }
            if games.is_empty() {
                warn(format!("HTTP {status} · 0 games · keys: {}", keys(value)))
            } else {
                ok(format!("HTTP {status} · {} game(s)", games.len()))
            }
        }
    );

    check!(
        rows, client,
        "games/v2/users/{id}/games",
        &p("games", &format!("/v2/users/{me}/games")),
        &[("limit", "10".to_string())],
        |status, value| array_like(status, value, &["data", "games"])
    );

    // -- catalog -----------------------------------------------------------
    check!(
        rows, client,
        "catalog/v1/search/items",
        &p("catalog", "/v1/search/items"),
        &[
            ("category", "All".to_string()),
            ("limit", "10".to_string()),
        ],
        |status, value| {
            let items = models::first_array(value, &["data"]);
            if let Some(entry) = items.first() {
                let parsed = models::CatalogItem::from_value(entry);
                if parsed.id != 0 {
                    asset = parsed.id;
                }
                if parsed.name.is_empty() {
                    // Expected: search answers with { id, itemType } stubs and the
                    // client hydrates names/prices through items/details.
                    return ok(format!(
                        "HTTP {status} · {} stub(s), names hydrated via items/details",
                        items.len()
                    ));
                }
            }
            if items.is_empty() {
                warn(format!("HTTP {status} · 0 items · keys: {}", keys(value)))
            } else {
                ok(format!("HTTP {status} · {} item(s)", items.len()))
            }
        }
    );

    check!(
        rows, client,
        "catalog/v1/search/navigation-menu-items",
        &p("catalog", "/v1/search/navigation-menu-items"),
        &[],
        |status, value| array_like(status, value, &["categories"])
    );

    if asset == 0 {
        rows.push(skipped(
            "catalog/v1/catalog/items/details",
            "no asset id to look up",
        ));
        rows.push(skipped("thumbnails/v1/assets", "no asset id to look up"));
    } else {
        check_post!(
            rows, client,
            "catalog/v1/catalog/items/details",
            &p("catalog", "/v1/catalog/items/details"),
            Some(serde_json::json!({ "items": [{ "itemType": "Asset", "id": asset }] })),
            |status, value| {
                let items = models::first_array(value, &["data"]);
                if items.is_empty() {
                    warn(format!(
                        "HTTP {status} · nothing for asset {asset} · keys: {}",
                        keys(value)
                    ))
                } else {
                    ok(format!("HTTP {status} · {} detail row(s)", items.len()))
                }
            }
        );

        check!(
            rows, client,
            "thumbnails/v1/assets",
            &p("thumbnails", "/v1/assets"),
            &[
                ("assetIds", asset.to_string()),
                ("size", "150x150".to_string()),
                ("format", "Png".to_string()),
            ],
            |status, value| thumbnail_like(status, value)
        );
    }

    // -- thumbnails --------------------------------------------------------
    check!(
        rows, client,
        "thumbnails/v1/users/avatar-headshot",
        &p("thumbnails", "/v1/users/avatar-headshot"),
        &[
            ("userIds", me.to_string()),
            ("size", "150x150".to_string()),
            ("format", "Png".to_string()),
        ],
        |status, value| thumbnail_like(status, value)
    );

    check!(
        rows, client,
        "thumbnails/v1/users/avatar",
        &p("thumbnails", "/v1/users/avatar"),
        &[
            ("userIds", me.to_string()),
            ("size", "420x420".to_string()),
            ("format", "Png".to_string()),
        ],
        |status, value| thumbnail_like(status, value)
    );

    if universe == 0 {
        rows.push(skipped(
            "thumbnails/v1/games/icons",
            "no universe id to probe",
        ));
    } else {
        check!(
            rows, client,
            "thumbnails/v1/games/icons",
            &p("thumbnails", "/v1/games/icons"),
            &[
                ("universeIds", universe.to_string()),
                ("size", "150x150".to_string()),
                ("format", "Png".to_string()),
            ],
            |status, value| thumbnail_like(status, value)
        );
    }

    // -- account information -----------------------------------------------
    check!(
        rows, client,
        "accountinformation/users/{id}/roblox-badges",
        &p("accountinformation", &format!("/v1/users/{me}/roblox-badges")),
        &[],
        |status, value| array_like(status, value, &["data"])
    );

    check!(
        rows, client,
        "accountinformation/users/{id}/badges",
        &p("accountinformation", &format!("/v1/users/{me}/badges")),
        &[],
        |status, value| array_like(status, value, &["data"])
    );

    check!(
        rows, client,
        "accountinformation/…/promotion-channels",
        &p(
            "accountinformation",
            &format!("/v1/users/{me}/promotion-channels")
        ),
        &[("alwaysReturnUrls", "true".to_string())],
        |status, value| object_like(status, value, "promotionChannelsVisibilityPrivacy")
    );

    check!(
        rows, client,
        "premiumfeatures/…/validate-membership",
        &p("premiumfeatures", &format!("/v1/users/{me}/validate-membership")),
        &[],
        |status, value| ok(format!("HTTP {status} · {}", summarise(value)))
    );

    // -- site meta ---------------------------------------------------------
    check!(
        rows, client,
        "api/alerts/alert-info",
        &p("api", "/alerts/alert-info"),
        &[],
        |status, value| ok(format!("HTTP {status} · {}", summarise(value)))
    );

    check!(
        rows, client,
        "auth/v1/maintenance/status",
        &p("auth", "/v1/maintenance/status"),
        &[],
        |status, value| {
            let on = models::bool_at(value, "isMaintenance") || models::bool_at(value, "maintenance");
            if on {
                warn(format!("HTTP {status} · the site reports maintenance mode"))
            } else {
                ok(format!("HTTP {status} · not in maintenance"))
            }
        }
    );

    check!(
        rows, client,
        "theme/v1/settings",
        &p("theme", "/v1/settings"),
        &[],
        |status, value| object_like(status, value, "modernThemeEnabled")
    );

    check!(
        rows, client,
        "events/v1/events",
        &p("events", "/v1/events"),
        &[],
        |status, value| array_like(status, value, &["data", "events"])
    );

    check!(
        rows, client,
        "events/v1/blogs",
        &p("events", "/v1/blogs"),
        &[],
        |status, value| array_like(status, value, &["data", "blogs"])
    );

    check!(
        rows, client,
        "contests/v1/metadata",
        &p("contests", "/v1/metadata"),
        &[],
        |status, value| ok(format!("HTTP {status} · {}", summarise(value)))
    );

    check!(
        rows, client,
        "develop/v1/place-creation/context",
        &p("develop", "/v1/place-creation/context"),
        &[],
        |status, value| ok(format!("HTTP {status} · {}", summarise(value)))
    );

    if asset == 0 {
        rows.push(skipped("api/v1/items/restrictions", "no asset id to probe"));
    } else {
        check!(
            rows, client,
            "api/v1/items/restrictions",
            &p("api", "/v1/items/restrictions"),
            &[("assetIds", asset.to_string())],
            |status, value| array_like(status, value, &["data"])
        );
    }

    check_soft!(
        rows, client,
        "ads/v1/user-ads/{id}",
        &p("ads", &format!("/v1/user-ads/{me}")),
        &[],
        |status, value| ok(format!("HTTP {status} · {}", summarise(value)))
    );

    check_post!(
        rows, client,
        "presence/v1/presence/users",
        &p("presence", "/v1/presence/users"),
        Some(serde_json::json!({ "userIds": [me] })),
        |status, value| {
            let presences = models::first_array(value, &["userPresences"]);
            if presences.is_empty() {
                warn(format!(
                    "HTTP {status} · no presence rows · keys: {}",
                    keys(value)
                ))
            } else {
                ok(format!("HTTP {status} · {} presence row(s)", presences.len()))
            }
        }
    );

    // -- client pipelines --------------------------------------------------
    // The functions the screens actually call, merges and all. A parser that
    // returns empty rows where the UI needs names shows up here.
    {
        let started = Instant::now();
        let query = endpoints::CatalogQuery::default();
        let (verdict, detail) = match endpoints::catalog_bundle(client, &query).await {
            Ok(bundle) => {
                let named = bundle.items.iter().filter(|item| !item.name.is_empty()).count();
                if bundle.items.is_empty() {
                    warn("no items to hydrate".to_string())
                } else if named == 0 {
                    (
                        Verdict::Fail,
                        format!(
                            "{} item(s) but none named — the details merge is broken",
                            bundle.items.len()
                        ),
                    )
                } else {
                    ok(format!(
                        "{} item(s) · {named} named · more={}",
                        bundle.items.len(),
                        bundle.next.is_some()
                    ))
                }
            }
            Err(error) => (Verdict::Fail, error.to_string()),
        };
        rows.push(Row {
            name: "pipeline · catalog (search+details)",
            detail,
            verdict,
            ms: started.elapsed().as_millis(),
        });
    }

    {
        let started = Instant::now();
        let (verdict, detail) = match endpoints::games_bundle(client, "", "").await {
            Ok(bundle) => {
                let named = bundle.games.iter().filter(|game| !game.name.is_empty()).count();
                let with_icon = bundle
                    .games
                    .iter()
                    .filter(|game| bundle.icons.contains_key(&game.id))
                    .count();
                if bundle.games.is_empty() {
                    warn("no games returned".to_string())
                } else if named == 0 {
                    (
                        Verdict::Fail,
                        format!("{} game(s) but none named", bundle.games.len()),
                    )
                } else {
                    ok(format!(
                        "{} game(s) · {named} named · {with_icon} with icon",
                        bundle.games.len()
                    ))
                }
            }
            Err(error) => (Verdict::Fail, error.to_string()),
        };
        rows.push(Row {
            name: "pipeline · games (list+icons)",
            detail,
            verdict,
            ms: started.elapsed().as_millis(),
        });
    }

    rows
}

// ---------------------------------------------------------------------------
// Assertions
// ---------------------------------------------------------------------------

fn ok(detail: String) -> (Verdict, String) {
    (Verdict::Pass, detail)
}

fn warn(detail: String) -> (Verdict, String) {
    (Verdict::Warn, detail)
}

fn fail(status: u16, value: &Value) -> (Verdict, String) {
    (Verdict::Fail, format!("HTTP {status} · keys: {}", keys(value)))
}

fn array_like(status: u16, value: &Value, keyset: &[&str]) -> (Verdict, String) {
    if value.is_null() {
        return ok(format!("HTTP {status} · empty body"));
    }
    // A bare `[]` is a valid, empty answer.
    if let Some(items) = value.as_array() {
        return ok(format!("HTTP {status} · {} item(s)", items.len()));
    }
    let items = models::first_array(value, keyset);
    if items.is_empty() && !keyset.iter().any(|key| value.get(*key).is_some()) {
        return fail(status, value);
    }
    ok(format!("HTTP {status} · {} item(s)", items.len()))
}

fn object_like(status: u16, value: &Value, expected: &str) -> (Verdict, String) {
    if value.get(expected).is_some() {
        ok(format!("HTTP {status} · has {expected}"))
    } else if value.is_object() {
        warn(format!(
            "HTTP {status} · object without {expected} · keys: {}",
            keys(value)
        ))
    } else {
        fail(status, value)
    }
}

fn count_like(status: u16, value: &Value) -> (Verdict, String) {
    match value.get("count") {
        Some(_) => ok(format!(
            "HTTP {status} · count={}",
            models::num_at(value, "count")
        )),
        None => fail(status, value),
    }
}

fn thumbnail_like(status: u16, value: &Value) -> (Verdict, String) {
    let rows = models::first_array(value, &["data"]);
    let mut with_url = 0;
    for row in rows {
        if !models::str_at(row, "imageUrl").is_empty() {
            with_url += 1;
        }
    }
    if rows.is_empty() {
        warn(format!(
            "HTTP {status} · 0 thumbnails · keys: {}",
            keys(value)
        ))
    } else if with_url == 0 {
        warn(format!(
            "HTTP {status} · {} row(s) but no imageUrl",
            rows.len()
        ))
    } else {
        ok(format!(
            "HTTP {status} · {with_url}/{} with imageUrl",
            rows.len()
        ))
    }
}

fn skipped(name: &'static str, reason: &str) -> Row {
    Row {
        name,
        detail: format!("skipped · {reason}"),
        verdict: Verdict::Warn,
        ms: 0,
    }
}

// ---------------------------------------------------------------------------
// Small formatting helpers
// ---------------------------------------------------------------------------

fn p(namespace: &str, rest: &str) -> String {
    format!("/apisite/{namespace}{rest}")
}

fn keys(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut names: Vec<&str> = map.keys().map(String::as_str).collect();
            names.sort_unstable();
            let shown: Vec<String> = names.iter().take(8).map(|name| name.to_string()).collect();
            if names.len() > shown.len() {
                format!("{}, …", shown.join(", "))
            } else {
                shown.join(", ")
            }
        }
        other => summarise(other),
    }
}

fn summarise(value: &Value) -> String {
    match value {
        Value::Null => "empty".to_string(),
        Value::Bool(flag) => format!("bool {flag}"),
        Value::Number(number) => format!("number {number}"),
        Value::String(text) => format!("string ({} chars)", text.chars().count()),
        Value::Array(items) => format!("array[{}]", items.len()),
        Value::Object(map) => format!("object with {} key(s)", map.len()),
    }
}

fn first(text: &str) -> String {
    text.chars().take(10).collect()
}
