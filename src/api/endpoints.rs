//! One wrapper per operation the dossier documents. Namespaces mirror the site's
//! `/apisite/{namespace}{path}` proxy, so an endpoint here is always
//! `api("<namespace>", "/v1/…")`.

use std::collections::HashMap;

use serde_json::{json, Value};

use super::models::*;
use super::{ApiClient, Result};

fn api(namespace: &str, path: &str) -> String {
    if path.starts_with('/') {
        format!("/apisite/{namespace}{path}")
    } else {
        format!("/apisite/{namespace}/{path}")
    }
}

// ---------------------------------------------------------------------------
// Users, economy, counters
// ---------------------------------------------------------------------------

pub async fn authenticated_user(client: &ApiClient) -> Result<AuthUser> {
    Ok(AuthUser::from_value(
        &client.get(&api("users", "/v1/users/authenticated")).await?,
    ))
}

pub async fn user(client: &ApiClient, id: u64) -> Result<UserProfile> {
    Ok(UserProfile::from_value(
        &client.get(&api("users", &format!("/v1/users/{id}"))).await?,
    ))
}

/// A player's Builders Club tier: 0 none, 1 Builders Club, 2 Turbo, 3 Outrageous.
pub async fn membership(client: &ApiClient, id: u64) -> Result<u8> {
    let value = client.get(&api("premiumfeatures", &format!("/v1/users/{id}/validate-membership"))).await?;
    Ok(value.as_u64().unwrap_or(0).min(3) as u8)
}

pub async fn user_status(client: &ApiClient, id: u64) -> Result<String> {
    let value = client
        .get(&api("users", &format!("/v1/users/{id}/status")))
        .await?;
    Ok(str_at(&value, "status"))
}

pub async fn username_history(client: &ApiClient, id: u64) -> Result<Vec<String>> {
    let value = client
        .get(&api("users", &format!("/v1/users/{id}/username-history")))
        .await?;
    Ok(second_array(&value, &["data"])
        .iter()
        .map(|entry| str_at(entry, "name"))
        .filter(|name| !name.is_empty())
        .collect())
}

pub async fn currency(client: &ApiClient, id: u64) -> Result<Currency> {
    Ok(Currency::from_value(
        &client
            .get(&api("economy", &format!("/v1/users/{id}/currency")))
            .await?,
    ))
}

/// The `transactionType` values the site's own Money page offers, with its labels
/// (the misspelt `commision` is the site's). `adrevenue` is in the site's picker
/// but the server answers it with HTTP 400.
pub const TRANSACTION_TYPES: [(&str, &str); 5] = [
    ("purchase", "Purchases"),
    ("sale", "Sales"),
    ("commision", "Commissions"),
    ("adrevenue", "Ad Revenue"),
    ("group-payout", "Group Payouts"),
];

/// Same query the site builds: `cursor` is always sent, empty for the first page.
pub async fn transactions(
    client: &ApiClient,
    id: u64,
    kind: &str,
    cursor: Option<&str>,
) -> Result<TransactionsPage> {
    let value = client
        .get_query(
            &api("economy", &format!("/v2/users/{id}/transactions")),
            &[
                ("cursor", cursor.unwrap_or_default().to_string()),
                ("transactionType", kind.to_string()),
            ],
        )
        .await?;
    let items: Vec<Transaction> = first_array(&value, &["data"])
        .iter()
        .map(Transaction::from_value)
        .collect();
    let mut ids: Vec<u64> = items
        .iter()
        .filter(|row| row.agent_id != 0 && !row.agent_type.eq_ignore_ascii_case("group"))
        .map(|row| row.agent_id)
        .collect();
    ids.sort_unstable();
    ids.dedup();
    let icons = headshots(client, &ids).await.unwrap_or_default();
    Ok(TransactionsPage {
        items,
        icons,
        next: value
            .get("nextPageCursor")
            .and_then(Value::as_str)
            .filter(|cursor| !cursor.is_empty())
            .map(str::to_string),
    })
}

/// The tabs on the site's Messages page. Inbox, Sent and Archive are `messageTab`
/// values (lowercase only; `Inbox` is answered with HTTP 500); News is the separate
/// announcements endpoint.
pub const MESSAGE_TABS: [(&str, &str); 4] = [
    ("inbox", "Inbox"),
    ("sent", "Sent"),
    ("news", "News"),
    ("archive", "Archive"),
];

pub const MESSAGE_PAGE_SIZE: u32 = 20;

/// Same query the site builds: `messageTab`, `pageSize` and a zero-based `pageNumber`.
pub async fn messages(client: &ApiClient, tab: &str, page: u32) -> Result<MessagesPage> {
    let value = if tab == "news" {
        client
            .get(&api("privatemessages", "/v1/announcements"))
            .await?
    } else {
        client
            .get_query(
                &api("privatemessages", "/v1/messages"),
                &[
                    ("messageTab", tab.to_string()),
                    ("pageSize", MESSAGE_PAGE_SIZE.to_string()),
                    ("pageNumber", page.to_string()),
                ],
            )
            .await?
    };
    let items: Vec<Message> = first_array(&value, &["collection", "data"])
        .iter()
        .map(Message::from_value)
        .collect();
    let total = num_at(&value, "totalCollectionSize").max(items.len() as u64);
    let mut ids: Vec<u64> = items
        .iter()
        .flat_map(|message| [message.sender_id, message.recipient_id])
        .filter(|id| *id != 0)
        .collect();
    ids.sort_unstable();
    ids.dedup();
    let icons = headshots(client, &ids).await.unwrap_or_default();
    Ok(MessagesPage { items, total, icons })
}

/// The tabs on the site's Trades page, which are also the path segment
/// (`/v1/trades/{tab}`). Anything else is answered with HTTP 400.
pub const TRADE_TABS: [(&str, &str); 4] = [
    ("inbound", "Inbound"),
    ("outbound", "Outbound"),
    ("completed", "Completed"),
    ("inactive", "Inactive"),
];

/// Same query the site builds: `cursor` is always sent, empty for the first page.
pub async fn trades(client: &ApiClient, tab: &str, cursor: Option<&str>) -> Result<TradesPage> {
    let value = client
        .get_query(
            &api("trades", &format!("/v1/trades/{tab}")),
            &[("cursor", cursor.unwrap_or_default().to_string())],
        )
        .await?;
    let items: Vec<TradeSummary> = first_array(&value, &["data"])
        .iter()
        .map(TradeSummary::from_value)
        .filter(|trade| trade.id != 0)
        .collect();
    let mut partners: Vec<u64> = items.iter().map(|trade| trade.partner_id).collect();
    partners.sort_unstable();
    partners.dedup();
    let icons = headshots(client, &partners).await.unwrap_or_default();
    Ok(TradesPage {
        items,
        icons,
        next: value
            .get("nextPageCursor")
            .and_then(Value::as_str)
            .filter(|cursor| !cursor.is_empty())
            .map(str::to_string),
    })
}

pub async fn trade_detail(client: &ApiClient, id: u64) -> Result<TradeDetail> {
    let value = client
        .get(&api("trades", &format!("/v1/trades/{id}")))
        .await?;
    let mut detail = TradeDetail::from_value(&value);
    let mut assets: Vec<u64> = detail
        .offers
        .iter()
        .flat_map(|offer| offer.assets.iter().map(|asset| asset.asset_id))
        .filter(|id| *id != 0)
        .collect();
    assets.sort_unstable();
    assets.dedup();
    detail.icons = asset_icons(client, &assets).await.unwrap_or_default();
    Ok(detail)
}

async fn count(client: &ApiClient, path: String) -> Result<u64> {
    Ok(num_at(&client.get(&path).await?, "count"))
}

pub async fn friend_request_count(client: &ApiClient) -> Result<u64> {
    count(client, api("friends", "/v1/user/friend-requests/count")).await
}

pub async fn unread_message_count(client: &ApiClient) -> Result<u64> {
    count(client, api("privatemessages", "/v1/messages/unread/count")).await
}

pub async fn inbound_trade_count(client: &ApiClient) -> Result<u64> {
    count(client, api("trades", "/v1/trades/inbound/count")).await
}

pub async fn follower_count(client: &ApiClient, id: u64) -> Result<u64> {
    count(client, api("friends", &format!("/v1/users/{id}/followers/count"))).await
}

pub async fn following_count(client: &ApiClient, id: u64) -> Result<u64> {
    count(client, api("friends", &format!("/v1/users/{id}/followings/count"))).await
}

pub async fn session(client: &ApiClient) -> Result<Session> {
    let user = authenticated_user(client).await?;
    let ids = [user.id];
    let (currency, requests, unread, trades, headshot) = tokio::join!(
        currency(client, user.id),
        friend_request_count(client),
        unread_message_count(client),
        inbound_trade_count(client),
        headshots(client, &ids),
    );
    Ok(Session {
        headshot: headshot.ok().and_then(|mut icons| icons.remove(&user.id)),
        counts: Counts {
            unread: unread.unwrap_or(0),
            friend_requests: requests.unwrap_or(0),
            trades: trades.unwrap_or(0),
        },
        currency: currency.unwrap_or_default(),
        user,
    })
}

// ---------------------------------------------------------------------------
// Thumbnails (always batched, exactly like the site)
// ---------------------------------------------------------------------------

async fn thumbnails(
    client: &ApiClient,
    endpoint: &str,
    parameter: &str,
    ids: &[u64],
    size: &str,
) -> Result<HashMap<u64, String>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let joined = ids
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let value = client
        .get_query(
            &api("thumbnails", &format!("/v1/{endpoint}")),
            &[
                (parameter, joined),
                ("size", size.to_string()),
                ("format", "Png".to_string()),
            ],
        )
        .await?;

    let mut icons = HashMap::new();
    for entry in first_array(&value, &["data"]) {
        let id = num_at(entry, "targetId");
        let url = str_at(entry, "imageUrl");
        if id != 0 && !url.is_empty() {
            icons.insert(id, url);
        }
    }
    Ok(icons)
}

pub async fn headshots(client: &ApiClient, ids: &[u64]) -> Result<HashMap<u64, String>> {
    thumbnails(client, "users/avatar-headshot", "userIds", ids, "150x150").await
}

pub async fn avatar_thumbnails(client: &ApiClient, ids: &[u64]) -> Result<HashMap<u64, String>> {
    thumbnails(client, "users/avatar", "userIds", ids, "420x420").await
}

pub async fn game_icons(client: &ApiClient, ids: &[u64]) -> Result<HashMap<u64, String>> {
    thumbnails(client, "games/icons", "universeIds", ids, "150x150").await
}

pub async fn group_icons(client: &ApiClient, ids: &[u64]) -> Result<HashMap<u64, String>> {
    thumbnails(client, "groups/icons", "groupIds", ids, "150x150").await
}

pub async fn asset_icons(client: &ApiClient, ids: &[u64]) -> Result<HashMap<u64, String>> {
    thumbnails(client, "assets", "assetIds", ids, "150x150").await
}

// ---------------------------------------------------------------------------
// Friends & presence
// ---------------------------------------------------------------------------

pub async fn friends(client: &ApiClient, id: u64) -> Result<Vec<Friend>> {
    let value = client
        .get(&api("friends", &format!("/v1/users/{id}/friends")))
        .await?;
    let mut friends: Vec<Friend> = first_array(&value, &["data", "friends"])
        .iter()
        .map(Friend::from_value)
        .collect();

    // The list's own `presenceType` is always 0 on this site; presence/v1 has the
    // real state. If that call fails the list's `isOnline` flag still stands.
    let ids: Vec<u64> = friends.iter().map(|friend| friend.id).collect();
    if let Ok(states) = presence_details(client, &ids).await {
        for friend in &mut friends {
            if let Some(state) = states.get(&friend.id) {
                friend.presence = state.kind;
                friend.is_online = state.kind > 0;
                friend.place_id = state.place_id;
                if !state.location.is_empty() {
                    friend.location = state.location.clone();
                }
            }
        }
    }
    Ok(friends)
}

pub async fn friends_bundle(client: &ApiClient, id: u64) -> Result<FriendsBundle> {
    let friends = friends(client, id).await?;
    let ids: Vec<u64> = friends.iter().map(|friend| friend.id).collect();
    let icons = headshots(client, &ids).await.unwrap_or_default();
    Ok(FriendsBundle { friends, icons })
}

/// Presence with the game: `placeId` and `lastLocation` while in a game. (Its
/// `gameId` is a number, not the server id the join script takes.)
#[derive(Debug, Clone, Default)]
pub struct PresenceDetail {
    pub kind: i64,
    pub place_id: Option<u64>,
    pub location: String,
    /// ISO time last seen online.
    pub last_online: String,
}

pub async fn presence_details(client: &ApiClient, ids: &[u64]) -> Result<HashMap<u64, PresenceDetail>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let body = json!({ "userIds": ids });
    let value = client
        .post(&api("presence", "/v1/presence/users"), Some(body))
        .await?;
    let mut presence = HashMap::new();
    for entry in first_array(&value, &["userPresences"]) {
        let kind = presence_number_of(entry);
        presence.insert(
            num_at(entry, "userId"),
            PresenceDetail {
                kind,
                place_id: if kind == 2 { opt_num_at(entry, "placeId").filter(|id| *id != 0) } else { None },
                location: str_at(entry, "lastLocation"),
                last_online: str_at(entry, "lastOnline"),
            },
        );
    }
    Ok(presence)
}

fn presence_number_of(value: &Value) -> i64 {
    match value.get("userPresenceType") {
        Some(Value::Number(number)) => number.as_i64().unwrap_or(0),
        Some(Value::String(text)) => match text.to_ascii_lowercase().as_str() {
            "online" => 1,
            "ingame" | "in-game" => 2,
            "instudio" => 3,
            _ => text.parse().unwrap_or(0),
        },
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Games
// ---------------------------------------------------------------------------

pub async fn game_sorts(client: &ApiClient) -> Result<Vec<(String, String)>> {
    let value = client
        .get_query(
            &api("games", "/v1/games/sorts"),
            &[("gameSortsContext", "GamesDefaultSorts".to_string())],
        )
        .await?;
    let mut sorts = Vec::new();
    for entry in first_array(&value, &["games", "sorts", "data"]) {
        let token = ["token", "sortToken", "id"]
            .iter()
            .find_map(|key| entry.get(*key).and_then(Value::as_str))
            .unwrap_or_default()
            .to_string();
        if token.is_empty() {
            continue;
        }
        let name = ["name", "displayName", "title"]
            .iter()
            .find_map(|key| entry.get(*key).and_then(Value::as_str))
            .unwrap_or("Sort")
            .to_string();
        sorts.push((token, name));
    }
    Ok(sorts)
}

pub async fn games(client: &ApiClient, sort: &str, keyword: &str, limit: u32) -> Result<Vec<Game>> {
    let mut query: Vec<(&str, String)> = vec![("maxRows", limit.to_string())];
    if !sort.is_empty() {
        query.push(("sortToken", sort.to_string()));
    }
    if !keyword.trim().is_empty() {
        query.push(("keyword", keyword.trim().to_string()));
    }
    let value = client
        .get_query(&api("games", "/v1/games/list"), &query)
        .await?;
    Ok(first_array(&value, &["games", "data"])
        .iter()
        .map(Game::from_value)
        .filter(|game| game.id != 0)
        .collect())
}

pub async fn games_bundle(client: &ApiClient, sort: &str, keyword: &str) -> Result<GamesBundle> {
    let games = games(client, sort, keyword, 24).await?;
    let ids: Vec<u64> = games.iter().map(|game| game.id).collect();
    let icons = game_icons(client, &ids).await.unwrap_or_default();
    Ok(GamesBundle { games, icons })
}

pub async fn user_games(client: &ApiClient, id: u64) -> Result<Vec<Game>> {
    let value = client
        .get_query(
            &api("games", &format!("/v2/users/{id}/games")),
            &[("limit", "50".to_string())],
        )
        .await?;
    Ok(first_array(&value, &["data", "games"])
        .iter()
        .map(Game::from_value)
        .filter(|game| game.id != 0)
        .collect())
}

// ---------------------------------------------------------------------------
// Catalog
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct CatalogQuery {
    pub keyword: String,
    pub category: String,
    pub sort: String,
    pub limit: u32,
    pub cursor: Option<String>,
}

impl Default for CatalogQuery {
    fn default() -> Self {
        Self {
            keyword: String::new(),
            category: "All".to_string(),
            sort: "Relevance".to_string(),
            limit: 28,
            cursor: None,
        }
    }
}

impl CatalogQuery {
    fn pairs(&self) -> Vec<(&'static str, String)> {
        let mut query = vec![
            ("category", self.category.clone()),
            ("limit", self.limit.to_string()),
            ("sortType", self.sort.clone()),
        ];
        if !self.keyword.trim().is_empty() {
            query.push(("keyword", self.keyword.trim().to_string()));
        }
        if let Some(cursor) = &self.cursor {
            if !cursor.is_empty() {
                query.push(("cursor", cursor.clone()));
            }
        }
        query
    }
}

/// Search returns only `{ id, itemType }` stubs, plus a cursor — the names and
/// prices come from the details endpoint, exactly as the site does it.
pub async fn catalog_search(
    client: &ApiClient,
    query: &CatalogQuery,
) -> Result<(Vec<CatalogItem>, Option<String>)> {
    let value = client
        .get_query(&api("catalog", "/v1/search/items"), &query.pairs())
        .await?;
    let items = first_array(&value, &["data"])
        .iter()
        .map(CatalogItem::from_value)
        .filter(|item| item.id != 0)
        .collect();
    let next = value
        .get("nextPageCursor")
        .and_then(Value::as_str)
        .filter(|cursor| !cursor.is_empty())
        .map(str::to_string);
    Ok((items, next))
}

pub async fn catalog_bundle(client: &ApiClient, query: &CatalogQuery) -> Result<CatalogBundle> {
    let (stubs, next) = catalog_search(client, query).await?;
    let ids: Vec<u64> = stubs.iter().map(|item| item.id).collect();
    let details = if ids.is_empty() {
        Vec::new()
    } else {
        catalog_details(client, &ids).await.unwrap_or_default()
    };

    let mut by_id: HashMap<u64, CatalogItem> = details
        .into_iter()
        .filter(|item| item.id != 0)
        .map(|item| (item.id, item))
        .collect();
    let items: Vec<CatalogItem> = stubs
        .into_iter()
        .map(|stub| match by_id.remove(&stub.id) {
            Some(mut full) => {
                if full.item_type.is_empty() {
                    full.item_type = stub.item_type;
                }
                full
            }
            None => stub,
        })
        .collect();

    let icon_ids: Vec<u64> = items.iter().map(|item| item.id).collect();
    let icons = asset_icons(client, &icon_ids).await.unwrap_or_default();
    Ok(CatalogBundle { items, icons, next })
}

pub async fn catalog_details(client: &ApiClient, ids: &[u64]) -> Result<Vec<CatalogItem>> {
    let items: Vec<Value> = ids
        .iter()
        .map(|id| json!({ "itemType": "Asset", "id": id }))
        .collect();
    let value = client
        .post(
            &api("catalog", "/v1/catalog/items/details"),
            Some(json!({ "items": items })),
        )
        .await?;
    Ok(first_array(&value, &["data"])
        .iter()
        .map(CatalogItem::from_value)
        .collect())
}

/// The real shape is `{ categories: [{ category, name, subcategories, … }] }`,
/// where `category` is the value the search endpoint expects.
pub async fn navigation_menu(client: &ApiClient) -> Result<Vec<(String, String)>> {
    let value = client
        .get(&api("catalog", "/v1/search/navigation-menu-items"))
        .await?;
    let mut entries = Vec::new();
    for item in first_array(&value, &["categories", "items", "data"]) {
        let category = str_at(item, "category");
        if category.is_empty() {
            continue;
        }
        let label = ["name", "displayName"]
            .iter()
            .find_map(|key| item.get(*key).and_then(Value::as_str))
            .unwrap_or(&category)
            .to_string();
        entries.push((category, label));
    }
    Ok(entries)
}

// ---------------------------------------------------------------------------
// Group clothing scrape + upload
// ---------------------------------------------------------------------------

/// Fetch all clothing items (T-Shirts, Shirts, Pants) uploaded by a Roblox group,
/// using Roblox's public catalog API directly. Paginates and enriches with full
/// item details and thumbnail URLs. Capped at 500 items.
pub async fn group_clothing(
    client: &ApiClient,
    group_id: u64,
) -> Result<(Vec<CatalogItem>, HashMap<u64, String>)> {
    let base: Vec<(&str, String)> = vec![
        ("category", "Clothing".to_string()),
        ("creatorType", "Group".to_string()),
        ("creatorTargetId", group_id.to_string()),
        ("limit", "120".to_string()),
        ("sortType", "Updated".to_string()),
    ];
    let mut all_stubs: Vec<CatalogItem> = Vec::new();
    let mut cursor = String::new();
    loop {
        let mut q = base.clone();
        if !cursor.is_empty() {
            q.push(("cursor", cursor.clone()));
        }
        let value = client
            .get_json_url("https://catalog.roblox.com/v1/search/items", &q)
            .await?;
        let page: Vec<CatalogItem> = first_array(&value, &["data"])
            .iter()
            .map(CatalogItem::from_value)
            .filter(|item| item.id != 0)
            .collect();
        all_stubs.extend(page);
        cursor = value
            .get("nextPageCursor")
            .and_then(Value::as_str)
            .filter(|c| !c.is_empty())
            .map(str::to_string)
            .unwrap_or_default();
        if cursor.is_empty() || all_stubs.len() >= 500 {
            break;
        }
    }

    // Enrich stubs with full detail via Roblox's catalog/items/details
    let ids: Vec<u64> = all_stubs.iter().map(|s| s.id).collect();
    let mut detail_map: HashMap<u64, CatalogItem> = HashMap::new();
    for chunk in ids.chunks(120) {
        let items_body: Vec<Value> = chunk
            .iter()
            .map(|id| json!({ "itemType": "Asset", "id": id }))
            .collect();
        if let Ok(value) = client
            .post_json_url(
                "https://catalog.roblox.com/v1/catalog/items/details",
                json!({ "items": items_body }),
            )
            .await
        {
            for item in first_array(&value, &["data"])
                .iter()
                .map(CatalogItem::from_value)
                .filter(|item| item.id != 0)
            {
                detail_map.insert(item.id, item);
            }
        }
    }
    let items: Vec<CatalogItem> = all_stubs
        .into_iter()
        .map(|stub| match detail_map.remove(&stub.id) {
            Some(mut full) => {
                if full.item_type.is_empty() {
                    full.item_type = stub.item_type;
                }
                full
            }
            None => stub,
        })
        .collect();

    // Thumbnails from Roblox's thumbnail API
    let icon_ids: Vec<u64> = items.iter().map(|item| item.id).collect();
    let icons = roblox_asset_icons(client, &icon_ids).await.unwrap_or_default();
    Ok((items, icons))
}

/// Thumbnails for asset IDs from Roblox's public thumbnails API.
async fn roblox_asset_icons(
    client: &ApiClient,
    ids: &[u64],
) -> Result<HashMap<u64, String>> {
    let mut map = HashMap::new();
    for chunk in ids.chunks(100) {
        let pairs: Vec<(&str, String)> = vec![
            ("assetIds", chunk.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",")),
            ("size", "110x110".to_string()),
            ("format", "Png".to_string()),
            ("isCircular", "false".to_string()),
        ];
        if let Ok(value) = client
            .get_json_url("https://thumbnails.roblox.com/v1/assets", &pairs)
            .await
        {
            for item in first_array(&value, &["data"]) {
                let id = item.get("targetId").and_then(Value::as_u64).unwrap_or(0);
                let url = str_at(item, "imageUrl");
                if id != 0 && !url.is_empty() {
                    map.insert(id, url);
                }
            }
        }
    }
    Ok(map)
}

/// Download raw clothing template bytes from Roblox's asset delivery CDN.
pub async fn download_asset(client: &ApiClient, asset_id: u64) -> Result<Vec<u8>> {
    client
        .fetch_image(&format!(
            "https://assetdelivery.roblox.com/v1/asset/?id={}",
            asset_id
        ))
        .await
}

/// Upload a clothing item to hexium. The endpoint is modelled from the classic
/// Roblox 2016 upload path (`POST /asset/`) — not observed in the traffic
/// capture; adjust if the actual path or response format differs.
/// Returns the new asset ID assigned by the server.
pub async fn upload_clothing(
    client: &ApiClient,
    name: &str,
    asset_type_id: u8,
    price: u64,
    image_bytes: Vec<u8>,
) -> Result<u64> {
    let query: &[(&str, String)] = &[
        ("name", name.to_string()),
        ("assetTypeId", asset_type_id.to_string()),
        ("allowComments", "True".to_string()),
        ("isPublic", "True".to_string()),
        ("isForSale", if price > 0 { "True" } else { "False" }.to_string()),
        ("priceInRobux", price.to_string()),
    ];
    let content_type = if image_bytes.starts_with(b"\xff\xd8") {
        "image/jpeg"
    } else {
        "image/png"
    };
    let text = client
        .post_bytes_text("/asset/", query, content_type, image_bytes)
        .await?;

    // Try JSON: {"AssetId": 12345} or {"assetId": 12345}
    if let Ok(value) = serde_json::from_str::<Value>(&text) {
        if let Some(id) = value
            .get("AssetId")
            .or_else(|| value.get("assetId"))
            .and_then(Value::as_u64)
        {
            return Ok(id);
        }
    }
    // Legacy text: "Asset ID: 12345"
    let lower = text.trim().to_ascii_lowercase();
    let stripped = lower.trim_start_matches("asset id:").trim();
    if let Ok(id) = stripped.parse::<u64>() {
        return Ok(id);
    }
    // Plain number
    if let Ok(id) = text.trim().parse::<u64>() {
        return Ok(id);
    }
    Err(super::ApiError::Parse(format!(
        "unexpected upload response: {}",
        &text[..text.len().min(200)]
    )))
}

// ---------------------------------------------------------------------------
// Avatar, inventory, groups, badges, events
// ---------------------------------------------------------------------------

pub async fn avatar_data(client: &ApiClient, id: u64) -> Result<AvatarData> {
    Ok(AvatarData::from_value(
        &client
            .get(&api("avatar", &format!("/v1/users/{id}/avatar")))
            .await?,
    ))
}

pub async fn avatar_bundle(client: &ApiClient, id: u64) -> Result<AvatarBundle> {
    let data = avatar_data(client, id).await?;
    let ids: Vec<u64> = data.assets.iter().map(|asset| asset.id).collect();
    let avatar_ids = [id];
    let (icons, render, palette) = tokio::join!(
        asset_icons(client, &ids),
        avatar_thumbnails(client, &avatar_ids),
        avatar_palette(client)
    );
    Ok(AvatarBundle {
        data,
        icons: icons.unwrap_or_default(),
        render: render.unwrap_or_default().remove(&id),
        palette: palette.unwrap_or_default(),
    })
}

pub async fn owned_asset_ids(client: &ApiClient, id: u64) -> Result<Vec<u64>> {
    let value = client
        .get(&api("inventory", &format!("/v1/users/{id}/owned-asset-ids")))
        .await?;
    let mut ids: Vec<u64> = first_array(&value, &["assetIds", "data"])
        .iter()
        .map(|entry| {
            entry
                .as_u64()
                .or_else(|| opt_num_at(entry, "assetId"))
                .or_else(|| opt_num_at(entry, "id"))
                .unwrap_or(0)
        })
        .filter(|id| *id != 0)
        .collect();
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

pub async fn collectibles(client: &ApiClient, id: u64) -> Result<Vec<Collectible>> {
    let value = client
        .get_query(
            &api("inventory", &format!("/v1/users/{id}/assets/collectibles")),
            &[("limit", "50".to_string())],
        )
        .await?;
    Ok(first_array(&value, &["data", "items"])
        .iter()
        .map(Collectible::from_value)
        .filter(|item| item.id != 0)
        .collect())
}

pub async fn inventory_bundle(client: &ApiClient, id: u64) -> Result<InventoryBundle> {
    let (owned, collectibles) = tokio::join!(owned_asset_ids(client, id), collectibles(client, id));
    let owned = owned.unwrap_or_default();
    let collectibles = collectibles.unwrap_or_default();
    let ids: Vec<u64> = collectibles.iter().map(|item| item.id).collect();
    let icons = asset_icons(client, &ids).await.unwrap_or_default();
    Ok(InventoryBundle {
        owned,
        collectibles,
        icons,
    })
}

pub async fn user_groups(client: &ApiClient, id: u64) -> Result<Vec<UserGroup>> {
    let value = client
        .get(&api("groups", &format!("/v2/users/{id}/groups/roles")))
        .await?;
    Ok(first_array(&value, &["data"])
        .iter()
        .map(UserGroup::from_value)
        .filter(|group| group.id != 0)
        .collect())
}

pub async fn groups_bundle(client: &ApiClient, id: u64) -> Result<GroupsBundle> {
    let groups = user_groups(client, id).await?;
    let ids: Vec<u64> = groups.iter().map(|group| group.id).collect();
    let icons = group_icons(client, &ids).await.unwrap_or_default();
    Ok(GroupsBundle { groups, icons })
}

pub async fn group_detail(client: &ApiClient, id: u64) -> Result<GroupDetail> {
    let info_path = api("groups", &format!("/v1/groups/{id}"));
    let roles_path = api("groups", &format!("/v1/groups/{id}/roles"));
    let wall_path = api("groups", &format!("/v2/groups/{id}/wall/posts"));
    let wall_query = [("limit", "20".to_string()), ("sortOrder", "Desc".to_string())];
    let (info, roles, wall) = tokio::join!(
        client.get(&info_path),
        client.get(&roles_path),
        client.get_query(&wall_path, &wall_query),
    );

    let info = GroupInfo::from_value(&info?);
    let roles = roles
        .map(|value| {
            first_array(&value, &["roles", "data"])
                .iter()
                .map(|role| (str_at(role, "name"), num_at(role, "memberCount")))
                .filter(|(name, _)| !name.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let wall = wall
        .map(|value| {
            first_array(&value, &["data"])
                .iter()
                .map(WallPost::from_value)
                .filter(|post| !post.body.trim().is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let icon = group_icons(client, &[id])
        .await
        .unwrap_or_default()
        .remove(&id);

    let mut posters: Vec<u64> = wall.iter().map(|post| post.poster_id).filter(|id| *id != 0).collect();
    posters.sort_unstable();
    posters.dedup();
    let wall_icons = headshots(client, &posters).await.unwrap_or_default();
    Ok(GroupDetail {
        info,
        roles,
        wall,
        icon,
        wall_icons,
    })
}

pub async fn roblox_badges(client: &ApiClient, id: u64) -> Result<Vec<Badge>> {
    let value = client
        .get(&api(
            "accountinformation",
            &format!("/v1/users/{id}/roblox-badges"),
        ))
        .await?;
    Ok(first_array(&value, &["data"])
        .iter()
        .map(Badge::from_value)
        .filter(|badge| !badge.name.is_empty())
        .collect())
}

pub async fn events(client: &ApiClient) -> Result<Vec<EventItem>> {
    let value = client.get(&api("events", "/v1/events")).await?;
    Ok(first_array(&value, &["data", "events"])
        .iter()
        .map(EventItem::from_value)
        .filter(|event| !event.title.is_empty())
        .collect())
}

pub async fn alert(client: &ApiClient) -> Result<Option<String>> {
    let value = client.get(&api("api", "/alerts/alert-info")).await?;
    let message = match &value {
        Value::String(text) => Some(text.clone()),
        Value::Object(map) => ["message", "alertMessage", "text", "content", "title"]
            .iter()
            .find_map(|key| map.get(*key).and_then(Value::as_str))
            .map(str::to_string),
        _ => None,
    };
    Ok(message.filter(|text| !text.trim().is_empty()))
}

pub async fn maintenance(client: &ApiClient) -> Result<Option<String>> {
    let value = client
        .get(&api("auth", "/v1/maintenance/status"))
        .await?;
    let active = bool_at(&value, "isMaintenance")
        || bool_at(&value, "maintenance")
        || bool_at(&value, "IsMaintenance");
    Ok(active.then(|| {
        let message = str_at(&value, "message");
        if message.is_empty() {
            "hexium.wtf reports scheduled maintenance.".to_string()
        } else {
            message
        }
    }))
}

// ---------------------------------------------------------------------------
// Composite screens
// ---------------------------------------------------------------------------

pub async fn profile_bundle(client: &ApiClient, id: u64) -> Result<ProfileBundle> {
    let avatar_ids = [id];
    let (user, status, friends, followers, following, badges, groups, games, avatar) = tokio::join!(
        user(client, id),
        user_status(client, id),
        friends(client, id),
        follower_count(client, id),
        following_count(client, id),
        roblox_badges(client, id),
        user_groups(client, id),
        user_games(client, id),
        avatar_thumbnails(client, &avatar_ids),
    );

    let friends = friends.unwrap_or_default();
    let groups = groups.unwrap_or_default();
    let games = games.unwrap_or_default();

    let friend_ids: Vec<u64> = friends.iter().map(|friend| friend.id).collect();
    let group_ids: Vec<u64> = groups.iter().map(|group| group.id).collect();
    let game_ids: Vec<u64> = games.iter().map(|game| game.id).collect();
    let (icons, group_icons, game_icons) = tokio::join!(
        headshots(client, &friend_ids),
        group_icons(client, &group_ids),
        game_icons(client, &game_ids),
    );

    Ok(ProfileBundle {
        user: user?,
        status: status.unwrap_or_default(),
        followers: followers.unwrap_or(0),
        following: following.unwrap_or(0),
        friends,
        icons: icons.unwrap_or_default(),
        badges: badges.unwrap_or_default(),
        groups,
        group_icons: group_icons.unwrap_or_default(),
        games,
        game_icons: game_icons.unwrap_or_default(),
        avatar: avatar.unwrap_or_default().remove(&id),
    })
}

/// `first_array` is for objects; this variant keeps a nested array usable by name.
fn second_array<'a>(value: &'a Value, keys: &[&str]) -> &'a [Value] {
    first_array(value, keys)
}

// ---------------------------------------------------------------------------
// Writes. Each one changes the signed-in account; the UI only calls them after
// the user confirms. Paths and bodies are the ones the site's own modules send.
// ---------------------------------------------------------------------------

/// `api-friends.js` `isFollowing`.
pub async fn is_following(client: &ApiClient, id: u64) -> Result<bool> {
    let value = client
        .post(
            &api("friends", "/v1/user/following-exists"),
            Some(json!({ "targetUserIds": [id] })),
        )
        .await?;
    Ok(first_array(&value, &["followings"])
        .iter()
        .find(|entry| num_at(entry, "userId") == id)
        .map(|entry| bool_at(entry, "isFollowing"))
        .unwrap_or(false))
}

/// `api-friends.js` `follow` / `unfollow`: a bare POST, no body.
pub async fn set_following(client: &ApiClient, id: u64, follow: bool) -> Result<()> {
    let action = if follow { "follow" } else { "unfollow" };
    client
        .post(&api("friends", &format!("/v1/users/{id}/{action}")), None)
        .await?;
    Ok(())
}

/// `api-catalog.js` `getAssetFavorite`: 204 with no body when the item is not a
/// favourite, an object when it is.
pub async fn is_favourite(client: &ApiClient, me: u64, asset: u64) -> Result<bool> {
    let value = client
        .get(&api("catalog", &format!("/v1/favorites/users/{me}/assets/{asset}/favorite")))
        .await?;
    Ok(value.is_object())
}

/// `api-catalog.js` `favoriteAsset` (POST) / `unfavoriteAsset` (DELETE).
pub async fn set_favourite(client: &ApiClient, me: u64, asset: u64, favourite: bool) -> Result<()> {
    let path = api("catalog", &format!("/v1/favorites/users/{me}/assets/{asset}/favorite"));
    if favourite {
        client.post(&path, None).await?;
    } else {
        client.delete(&path).await?;
    }
    Ok(())
}

/// `api-users.js` `setUserStatus`: PATCH with `{ status }`.
pub async fn set_status(client: &ApiClient, me: u64, status: &str) -> Result<()> {
    client
        .patch(
            &api("users", &format!("/v1/users/{me}/status")),
            Some(json!({ "status": status })),
        )
        .await?;
    Ok(())
}

/// `api-messages.js` `setReadStatus`: POST `mark-read` / `mark-unread`.
pub async fn set_messages_read(client: &ApiClient, ids: &[u64], read: bool) -> Result<()> {
    let action = if read { "mark-read" } else { "mark-unread" };
    client
        .post(
            &api("privatemessages", &format!("/v1/messages/{action}")),
            Some(json!({ "messageIds": ids })),
        )
        .await?;
    Ok(())
}

/// `api-friends.js` `getFriendRequests`; the shell asks for 10 with an empty cursor.
pub async fn friend_requests(client: &ApiClient) -> Result<FriendRequestsBundle> {
    let value = client
        .get_query(
            &api("friends", "/v1/my/friends/requests"),
            &[("limit", "50".to_string()), ("cursor", String::new())],
        )
        .await?;
    let items: Vec<FriendRequest> = first_array(&value, &["data"])
        .iter()
        .map(FriendRequest::from_value)
        .filter(|request| request.user_id != 0)
        .collect();
    let ids: Vec<u64> = items.iter().map(|request| request.user_id).collect();
    let icons = headshots(client, &ids).await.unwrap_or_default();
    Ok(FriendRequestsBundle { items, icons })
}

/// `api-friends.js` `getFriendshipStatus`: `Friends`, `NotFriends`, `RequestSent` or
/// `RequestReceived`. The site falls back to `NotFriends`; this returns the error
/// instead so the UI never offers the wrong button.
pub async fn friendship_status(client: &ApiClient, me: u64, id: u64) -> Result<String> {
    let value = client
        .get_query(
            &api("friends", &format!("/v1/users/{me}/friends/statuses")),
            &[("userIds", id.to_string())],
        )
        .await?;
    Ok(first_array(&value, &["data"])
        .iter()
        .find(|entry| num_at(entry, "id") == id)
        .map(|entry| str_at(entry, "status"))
        .filter(|status| !status.is_empty())
        .unwrap_or_else(|| "NotFriends".to_string()))
}

/// The four friendship writes in `api-friends.js`; each is a bare POST.
pub async fn friendship_write(client: &ApiClient, id: u64, action: &str) -> Result<()> {
    debug_assert!(matches!(
        action,
        "request-friendship" | "accept-friend-request" | "decline-friend-request" | "unfriend"
    ));
    client
        .post(&api("friends", &format!("/v1/users/{id}/{action}")), None)
        .await?;
    Ok(())
}

/// `api-messages.js` `sendMessage`. The field really is `recipientid` (lower-case
/// i) in the site's own code. Roblox-style send endpoints can answer 200 with
/// `success: false`, so that is treated as a failure too.
pub async fn send_message(
    client: &ApiClient,
    recipient: u64,
    subject: &str,
    body: &str,
    reply_to: Option<u64>,
    include_previous: bool,
) -> Result<()> {
    let mut payload = json!({
        "recipientid": recipient,
        "subject": subject,
        "body": body,
    });
    if let Some(id) = reply_to {
        payload["replyMessageId"] = json!(id);
        payload["includePreviousMessage"] = json!(include_previous);
    }
    let value = client
        .post(&api("privatemessages", "/v1/messages/send"), Some(payload))
        .await?;
    if value.get("success").and_then(Value::as_bool) == Some(false) {
        let reason = optional_text_any(&value, &["message", "shortMessage"])
            .unwrap_or_else(|| "the site said the message was not sent".to_string());
        return Err(super::ApiError::Rejected(200, reason));
    }
    Ok(())
}

fn optional_text_any(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .filter(|text| !text.trim().is_empty())
        .map(str::to_string)
}

/// The body colour palette the site offers, keyed by BrickColor id. Both the full
/// and the basic palette are merged, so any colour a body part can have is named.
pub async fn avatar_palette(client: &ApiClient) -> Result<HashMap<u64, PaletteColour>> {
    let value = client.get(&api("avatar", "/v1/avatar-rules")).await?;
    let mut palette = HashMap::new();
    for key in ["bodyColorsPalette", "basicBodyColorsPalette"] {
        for entry in first_array(&value, &[key]) {
            let id = num_at(entry, "brickColorId");
            let hex = str_at(entry, "hexColor");
            if id != 0 && hex.starts_with('#') {
                palette.entry(id).or_insert(PaletteColour {
                    hex,
                    name: str_at(entry, "name"),
                });
            }
        }
    }
    Ok(palette)
}

/// `api-trades.js` `acceptTrade` / `declineTrade`: a bare POST. Accepting moves the
/// items at once; neither can be undone.
pub async fn answer_trade(client: &ApiClient, id: u64, accept: bool) -> Result<()> {
    let action = if accept { "accept" } else { "decline" };
    client
        .post(&api("trades", &format!("/v1/trades/{id}/{action}")), None)
        .await?;
    Ok(())
}

/// Your listing for one copy, the cheapest other listing, and the item's sales.
/// Uses `getResellableCopies`, `getAssetResellers` and `getResaleData` from
/// `api-economy.js`. A price of 0 means the copy is not for sale.
pub async fn resale_info(client: &ApiClient, me: u64, asset: u64, user_asset_id: u64) -> Result<ResaleInfo> {
    let copies_path = api("economy", &format!("/v1/assets/{asset}/users/{me}/resellable-copies"));
    let resellers_path = api("economy", &format!("/v1/assets/{asset}/resellers"));
    let resellers_query = [("limit", "100".to_string()), ("cursor", String::new())];
    let data_path = api("economy", &format!("/v1/assets/{asset}/resale-data"));
    let (copies, resellers, data) = tokio::join!(
        client.get(&copies_path),
        client.get_query(&resellers_path, &resellers_query),
        client.get(&data_path),
    );
    let copies = copies?;
    let my_price = first_array(&copies, &["data"])
        .iter()
        .find(|copy| num_at(copy, "userAssetId") == user_asset_id)
        .map(|copy| num_at(copy, "price"))
        .filter(|price| *price > 0);

    let mut lowest_other = None;
    let mut other_sellers = 0;
    if let Ok(resellers) = &resellers {
        for listing in first_array(resellers, &["data"]) {
            let seller = listing.get("seller").map(|seller| num_at(seller, "id")).unwrap_or(0);
            let price = num_at(listing, "price");
            if seller == me || price == 0 {
                continue;
            }
            other_sellers += 1;
            lowest_other = Some(lowest_other.map_or(price, |lowest: u64| lowest.min(price)));
        }
    }
    let (sales, rap) = match &data {
        Ok(data) => (num_at(data, "sales"), num_at(data, "recentAveragePrice")),
        Err(_) => (0, 0),
    };
    // The same answer carries the per-day history for the chart.
    let history = data.as_ref().ok().map(|data| ResaleData::from_value(asset, data));
    Ok(ResaleInfo {
        my_price,
        lowest_other,
        other_sellers,
        sales,
        rap,
        history,
    })
}

/// `api-economy.js` `setResalePrice` / `removeResaleListing`: PATCH with
/// `{ price }`, where 0 takes the copy off sale.
pub async fn set_resale_price(client: &ApiClient, asset: u64, user_asset_id: u64, price: u64) -> Result<()> {
    client
        .patch(
            &api("economy", &format!("/v1/assets/{asset}/resellable-copies/{user_asset_id}")),
            Some(json!({ "price": price })),
        )
        .await?;
    Ok(())
}

/// `api-messages.js` `setArchivedStatus`: POST `archive` / `unarchive` with
/// `{ messageIds }`.
pub async fn set_messages_archived(client: &ApiClient, ids: &[u64], archive: bool) -> Result<()> {
    let action = if archive { "archive" } else { "unarchive" };
    client
        .post(
            &api("privatemessages", &format!("/v1/messages/{action}")),
            Some(json!({ "messageIds": ids })),
        )
        .await?;
    Ok(())
}

/// Every unread message id in the inbox, across all pages, not just the ones on
/// screen. Stops after 50 pages (1,000 messages) as a safety limit.
pub async fn unread_inbox_ids(client: &ApiClient) -> Result<Vec<u64>> {
    let mut ids = Vec::new();
    let mut seen = 0u64;
    for page in 0..50 {
        let batch = messages(client, "inbox", page).await?;
        if batch.items.is_empty() {
            break;
        }
        seen += batch.items.len() as u64;
        ids.extend(batch.items.iter().filter(|message| !message.is_read).map(|message| message.id));
        if seen >= batch.total {
            break;
        }
    }
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

/// Marks many messages read in batches of 20, one request at a time. If a batch
/// fails, the error says how many were already marked.
pub async fn mark_all_read(client: &ApiClient, ids: &[u64]) -> Result<()> {
    let mut done = 0usize;
    for chunk in ids.chunks(20) {
        if let Err(error) = set_messages_read(client, chunk, true).await {
            if done == 0 {
                return Err(error);
            }
            return Err(super::ApiError::Partial(format!(
                "Marked {done} of {} as read, then the next batch failed: {error}",
                ids.len()
            )));
        }
        done += chunk.len();
    }
    Ok(())
}

/// Counts from `getGameVotes` plus `/v1/games/{id}/votes/user`, which says
/// whether you may vote and how you already did.
pub async fn game_vote_state(client: &ApiClient, universe: u64) -> Result<GameVoteState> {
    let votes_path = api("games", "/v1/games/votes");
    let votes_query = [("universeIds", universe.to_string())];
    let user_path = api("games", &format!("/v1/games/{universe}/votes/user"));
    let (votes, user) = tokio::join!(
        client.get_query(&votes_path, &votes_query),
        client.get(&user_path),
    );
    let votes = votes?;
    let entry = first_array(&votes, &["data"])
        .iter()
        .find(|entry| num_at(entry, "id") == universe)
        .cloned()
        .unwrap_or(Value::Null);
    let mut state = GameVoteState {
        up: num_at(&entry, "upVotes"),
        down: num_at(&entry, "downVotes"),
        user_vote: entry.get("userVote").and_then(Value::as_bool),
        can_vote: true,
        reason: None,
    };
    if let Ok(user) = user {
        if let Some(can) = user.get("canVote").and_then(Value::as_bool) {
            state.can_vote = can;
        }
        if let Some(vote) = user.get("userVote").and_then(Value::as_bool) {
            state.user_vote = Some(vote);
        }
        state.reason = user
            .get("reasonForNotVoteable")
            .and_then(Value::as_str)
            .filter(|reason| !reason.trim().is_empty())
            .map(str::to_string);
    }
    Ok(state)
}

/// `api-games.js` `voteGame`: PATCH `{ vote: true | false }`.
pub async fn vote_game(client: &ApiClient, universe: u64, up: bool) -> Result<()> {
    client
        .patch(
            &api("games", &format!("/v1/games/{universe}/user-votes")),
            Some(json!({ "vote": up })),
        )
        .await?;
    Ok(())
}

/// `api-account-settings.js` `setDescription`: POST `{ description }`.
pub async fn set_description(client: &ApiClient, description: &str) -> Result<()> {
    client
        .post(
            &api("accountinformation", "/v1/description"),
            Some(json!({ "description": description })),
        )
        .await?;
    Ok(())
}

/// Every collectible a user owns, following the cursor (some users have hundreds),
/// with thumbnails. Stops after 10 pages of 100 as a safety limit.
pub async fn trade_inventory(client: &ApiClient, id: u64) -> Result<TradeInventory> {
    let path = api("inventory", &format!("/v1/users/{id}/assets/collectibles"));
    let mut items: Vec<Collectible> = Vec::new();
    let mut cursor = String::new();
    for _ in 0..10 {
        let value = client
            .get_query(&path, &[("limit", "100".to_string()), ("cursor", cursor.clone())])
            .await?;
        items.extend(
            first_array(&value, &["data"])
                .iter()
                .map(Collectible::from_value)
                .filter(|item| item.id != 0 && item.user_asset_id != 0),
        );
        match value.get("nextPageCursor").and_then(Value::as_str) {
            Some(next) if !next.is_empty() => cursor = next.to_string(),
            _ => break,
        }
    }
    items.sort_by(|a, b| b.rap.cmp(&a.rap).then_with(|| a.name.cmp(&b.name)));

    let mut asset_ids: Vec<u64> = items.iter().map(|item| item.id).collect();
    asset_ids.sort_unstable();
    asset_ids.dedup();
    let mut icons = HashMap::new();
    for chunk in asset_ids.chunks(100) {
        if let Ok(batch) = asset_icons(client, chunk).await {
            icons.extend(batch);
        }
    }
    Ok(TradeInventory { items, icons })
}

/// The `offers` body for send and counter. `robux` is left out of a side that
/// adds none: the site's trade window sends it undefined (so the key is dropped),
/// and this server answers a present `0` with "Cannot add negative Robux amounts
/// to a trade" (code 11). Trades without Robux are stored with `robux: null`.
pub fn trade_body(me: u64, partner: u64, give: &[u64], get: &[u64], give_robux: u64, get_robux: u64) -> Value {
    let side = |user: u64, assets: &[u64], robux: u64| {
        let mut offer = json!({ "userAssetIds": assets, "userId": user });
        if robux > 0 {
            offer["robux"] = json!(robux);
        }
        offer
    };
    json!({ "offers": [side(me, give, give_robux), side(partner, get, get_robux)] })
}

/// A counter's body: the same two sides, the partner's first.
pub fn counter_body(me: u64, partner: u64, give: &[u64], get: &[u64], give_robux: u64, get_robux: u64) -> Value {
    let mut body = trade_body(me, partner, give, get, give_robux, get_robux);
    if let Some(offers) = body["offers"].as_array_mut() {
        offers.reverse();
    }
    body
}

/// `api-trades.js` `sendTrade` / `counterTrade`, with optional Robux on either side
/// (the site's `offerRobux` / `requestRobux`).
///
/// A new trade lists your offer first. A counter keeps the original trade's
/// order, the original sender (your partner) first: with your side first this
/// server answers "One or more userAssets are invalid" (code 12) even though
/// every id is a real copy owned by the user named beside it.
pub async fn send_trade(
    client: &ApiClient,
    me: u64,
    partner: u64,
    give: &[u64],
    get: &[u64],
    give_robux: u64,
    get_robux: u64,
    counter_of: Option<u64>,
) -> Result<()> {
    let body = match counter_of {
        Some(_) => counter_body(me, partner, give, get, give_robux, get_robux),
        None => trade_body(me, partner, give, get, give_robux, get_robux),
    };
    let path = match counter_of {
        Some(id) => api("trades", &format!("/v1/trades/{id}/counter")),
        None => api("trades", "/v1/trades/send"),
    };
    client.post(&path, Some(body)).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Chat (`chat/v2`, as used by `app-shell-chat-signalr.js`)
// ---------------------------------------------------------------------------

/// Conversations, friends without one, headshots and the chat switch.
pub async fn chat_bundle(client: &ApiClient, me: u64) -> Result<ChatBundle> {
    let query = [("pageNumber", "1".to_string()), ("pageSize", "100".to_string())];
    let conversations_path = api("chat", "/v2/get-user-conversations");
    let settings_path = api("chat", "/v2/chat-settings");
    let (conversations, settings, friend_list) = tokio::join!(
        client.get_query(&conversations_path, &query),
        client.get(&settings_path),
        friends(client, me),
    );
    let mut conversations: Vec<ChatConversation> =
        first_array(&conversations?, &["data", "conversations"])
            .iter()
            .map(ChatConversation::from_value)
            .filter(|conversation| conversation.id != 0)
            .collect();

    // The list carries no previews; the site fetches them separately.
    if !conversations.is_empty() {
        let ids: Vec<String> = conversations.iter().map(|conversation| conversation.id.to_string()).collect();
        let path = api("chat", "/v2/multi-get-latest-messages");
        let query = [("conversationIds", ids.join(",")), ("pageSize", "1".to_string())];
        if let Ok(value) = client.get_query(&path, &query).await {
            for entry in first_array(&value, &["data"]) {
                let id = num_at(entry, "conversationId");
                let latest = arr_at(entry, "chatMessages").first().map(ChatMessage::from_value);
                if let Some(conversation) = conversations.iter_mut().find(|conversation| conversation.id == id) {
                    if latest.is_some() {
                        conversation.latest = latest;
                    }
                }
            }
        }
        // Newest first, as on the site.
        conversations.sort_by(|a, b| {
            let when = |conversation: &ChatConversation| {
                conversation.latest.as_ref().map(|latest| latest.sent.clone()).unwrap_or_default()
            };
            when(b).cmp(&when(a))
        });
    }
    // Default to enabled when the settings call fails: the list still loaded.
    let chat_enabled = settings
        .ok()
        .and_then(|value| value.get("chatEnabled").and_then(Value::as_bool))
        .unwrap_or(true);

    let talking_to: Vec<u64> = conversations
        .iter()
        .filter(|conversation| {
            conversation.kind.is_empty() || conversation.kind == "OneToOneConversation"
        })
        .filter_map(|conversation| conversation.other(me).map(|other| other.id))
        .collect();
    let friend_list: Vec<Friend> = friend_list
        .unwrap_or_default()
        .into_iter()
        .filter(|friend| !talking_to.contains(&friend.id))
        .collect();

    let mut ids: Vec<u64> = conversations
        .iter()
        .flat_map(|conversation| conversation.participants.iter().map(|participant| participant.id))
        .chain(friend_list.iter().map(|friend| friend.id))
        .filter(|id| *id != 0 && *id != me)
        .collect();
    ids.sort_unstable();
    ids.dedup();
    let icons = headshots(client, &ids).await.unwrap_or_default();
    Ok(ChatBundle {
        conversations,
        friends: friend_list,
        icons,
        chat_enabled,
    })
}

/// The newest 100 messages of a conversation, oldest first.
pub async fn chat_messages(client: &ApiClient, conversation: u64) -> Result<Vec<ChatMessage>> {
    let query = [
        ("conversationId", conversation.to_string()),
        ("pageSize", "100".to_string()),
        ("exclusiveStartMessageId", String::new()),
    ];
    let path = api("chat", "/v2/get-messages");
    let value = client.get_query(&path, &query).await?;
    let mut messages: Vec<ChatMessage> = first_array(&value, &["data", "messages"])
        .iter()
        .map(ChatMessage::from_value)
        .collect();
    // The site's list is newest first.
    messages.reverse();
    Ok(messages)
}

/// `mark-as-read`, which the site itself sends whenever a conversation is open.
/// It only changes read state, so it is not treated as a confirmed write.
pub async fn chat_mark_read(client: &ApiClient, conversation: u64, end_message: &str) -> Result<()> {
    let end: Value = end_message
        .parse::<u64>()
        .map(Value::from)
        .unwrap_or_else(|_| Value::from(end_message));
    client
        .post(
            &api("chat", "/v2/mark-as-read"),
            Some(json!({ "conversationId": conversation, "endMessageId": end })),
        )
        .await?;
    Ok(())
}

/// `update-user-typing-status`: the site sends `isTyping: true` once a second
/// while you type and never sends `false`; the other side times it out.
pub async fn chat_typing(client: &ApiClient, conversation: u64) -> Result<()> {
    client
        .post(
            &api("chat", "/v2/update-user-typing-status"),
            Some(json!({ "isTyping": true, "conversationId": conversation })),
        )
        .await?;
    Ok(())
}

/// `start-one-to-one-conversation`: returns the conversation (new or existing).
pub async fn chat_start(client: &ApiClient, user: u64) -> Result<ChatConversation> {
    let value = client
        .post(
            &api("chat", "/v2/start-one-to-one-conversation"),
            Some(json!({ "participantUserId": user })),
        )
        .await?;
    let conversation = ChatConversation::from_value(value.get("conversation").unwrap_or(&value));
    if conversation.id == 0 {
        let reason = optional_text_any(&value, &["statusMessage", "resultType", "message"])
            .unwrap_or_else(|| "the site did not return a conversation".to_string());
        return Err(super::ApiError::Rejected(200, reason));
    }
    Ok(conversation)
}

/// `send-message`. A 200 whose `resultType` is not `Success` (moderated, flood
/// checked, …) is a failure.
pub async fn chat_send(client: &ApiClient, conversation: u64, text: &str) -> Result<ChatMessage> {
    let value = client
        .post(
            &api("chat", "/v2/send-message"),
            Some(json!({ "conversationId": conversation, "message": text })),
        )
        .await?;
    if let Some(result) = value.get("resultType").and_then(Value::as_str) {
        if !result.eq_ignore_ascii_case("success") {
            let reason =
                optional_text_any(&value, &["statusMessage"]).unwrap_or_else(|| result.to_string());
            return Err(super::ApiError::Rejected(200, reason));
        }
    }
    Ok(ChatMessage::from_value(&value))
}

// ---------------------------------------------------------------------------
// Game servers
// ---------------------------------------------------------------------------

/// `getGameInstances` in `api-games.js`: running servers of a place, a page at a
/// time from `start`.
pub async fn game_servers(client: &ApiClient, place: u64, start: u64) -> Result<ServerPage> {
    let query = [("placeId", place.to_string()), ("startIndex", start.to_string())];
    let value = client.get_query("/games/getgameinstancesjson", &query).await?;
    let servers: Vec<GameServer> = arr_at(&value, "Collection")
        .iter()
        .map(GameServer::from_value)
        .filter(|server| !server.job_id.is_empty())
        .collect();
    let mut servers = servers;
    // The listed thumbnails are relative redirects; use the thumbnails API.
    let ids: Vec<u64> = servers
        .iter()
        .flat_map(|server| server.players.iter().map(|player| player.id))
        .filter(|id| *id != 0)
        .collect();
    let icons = headshots(client, &ids).await.unwrap_or_default();
    for player in servers.iter_mut().flat_map(|server| server.players.iter_mut()) {
        player.headshot = icons
            .get(&player.id)
            .cloned()
            .or_else(|| player.headshot.take().filter(|url| url.starts_with("http")));
    }
    Ok(ServerPage {
        place_id: place,
        total: opt_num_at(&value, "TotalCollectionSize").unwrap_or(servers.len() as u64),
        servers,
    })
}

/// The server a user is playing on, found by walking the place's server list
/// (at most 20 pages).
pub async fn find_server_of(client: &ApiClient, place: u64, user: u64) -> Result<Option<String>> {
    let mut start = 0u64;
    for _ in 0..20 {
        let query = [("placeId", place.to_string()), ("startIndex", start.to_string())];
        let value = client.get_query("/games/getgameinstancesjson", &query).await?;
        let servers: Vec<GameServer> = arr_at(&value, "Collection").iter().map(GameServer::from_value).collect();
        if let Some(server) = servers
            .iter()
            .find(|server| server.players.iter().any(|player| player.id == user))
        {
            return Ok(Some(server.job_id.clone()));
        }
        let total = opt_num_at(&value, "TotalCollectionSize").unwrap_or(0);
        start += servers.len() as u64;
        if servers.is_empty() || start >= total {
            break;
        }
    }
    Ok(None)
}

// ---------------------------------------------------------------------------
// Resellers
// ---------------------------------------------------------------------------

/// One page of a limited's resellers, cheapest first, with seller headshots.
pub async fn resellers(client: &ApiClient, asset: u64, cursor: Option<&str>) -> Result<ResellersPage> {
    let query = [("limit", "100".to_string()), ("cursor", cursor.unwrap_or_default().to_string())];
    let value = client
        .get_query(&api("economy", &format!("/v1/assets/{asset}/resellers")), &query)
        .await?;
    let mut items: Vec<Reseller> = first_array(&value, &["data"])
        .iter()
        .map(Reseller::from_value)
        .filter(|item| item.price > 0)
        .collect();
    sort_resellers(&mut items);
    let mut ids: Vec<u64> = items.iter().map(|item| item.seller_id).filter(|id| *id != 0).collect();
    ids.sort_unstable();
    ids.dedup();
    let icons = headshots(client, &ids).await.unwrap_or_default();
    Ok(ResellersPage {
        asset_id: asset,
        items,
        icons,
        next: value
            .get("nextPageCursor")
            .and_then(Value::as_str)
            .filter(|cursor| !cursor.is_empty())
            .map(str::to_string),
    })
}

/// RAP, sales and the per-day price and volume history of a limited.
pub async fn resale_data(client: &ApiClient, asset: u64) -> Result<ResaleData> {
    let value = client
        .get(&api("economy", &format!("/v1/assets/{asset}/resale-data")))
        .await?;
    Ok(ResaleData::from_value(asset, &value))
}

#[cfg(test)]
mod trade_tests {
    use super::{counter_body, trade_body};

    /// A present `"robux": 0` is refused by the server; no Robux means no key.
    #[test]
    fn no_robux_key_without_robux() {
        let body = trade_body(1, 2, &[10, 11], &[20], 0, 0);
        let offers = body["offers"].as_array().unwrap();
        assert_eq!(offers.len(), 2);
        assert!(offers.iter().all(|offer| offer.get("robux").is_none()));
        assert_eq!(offers[0]["userId"], 1);
        assert_eq!(offers[0]["userAssetIds"], serde_json::json!([10, 11]));
        assert_eq!(offers[1]["userId"], 2);

        let with = trade_body(1, 2, &[10], &[20], 50, 0);
        assert_eq!(with["offers"][0]["robux"], 50);
        assert!(with["offers"][1].get("robux").is_none());
    }

    /// A counter puts the partner's side first; each side keeps its own items.
    #[test]
    fn counter_puts_the_partner_first() {
        let body = counter_body(1, 2, &[10, 11], &[20], 0, 0);
        assert_eq!(body["offers"][0]["userId"], 2);
        assert_eq!(body["offers"][0]["userAssetIds"], serde_json::json!([20]));
        assert_eq!(body["offers"][1]["userId"], 1);
        assert_eq!(body["offers"][1]["userAssetIds"], serde_json::json!([10, 11]));
        assert!(body["offers"][0].get("robux").is_none());

        // Robux stays with its own side when the order flips.
        let paid = counter_body(1, 2, &[10], &[20], 30, 0);
        assert!(paid["offers"][0].get("robux").is_none());
        assert_eq!(paid["offers"][1]["robux"], 30);
    }
}

/// The cheapest reseller's price of a limited (`None` when nobody sells one).
pub async fn cheapest_resale(client: &ApiClient, asset: u64) -> Result<Option<u64>> {
    let query = [("limit", "10".to_string()), ("cursor", String::new())];
    let value = client
        .get_query(&api("economy", &format!("/v1/assets/{asset}/resellers")), &query)
        .await?;
    Ok(first_array(&value, &["data"])
        .iter()
        .map(|listing| num_at(listing, "price"))
        .filter(|price| *price > 0)
        .min())
}

// ---------------------------------------------------------------------------
// Who owns copies of an item (`inventory/v2/assets/{id}/owners`)
// ---------------------------------------------------------------------------

/// One copy and its owner.
#[derive(Debug, Clone, PartialEq)]
pub struct AssetOwner {
    /// The copy (user asset id).
    pub user_asset_id: u64,
    pub user_id: u64,
    pub name: String,
    pub serial: Option<u64>,
    /// When this owner got the copy (ISO time).
    pub updated: String,
}

/// Every listed copy of an item, up to `max_pages` pages of 100. Copies whose
/// owner is hidden or not a user are left out.
pub async fn asset_owners(client: &ApiClient, asset: u64, max_pages: usize) -> Result<Vec<AssetOwner>> {
    let path = api("inventory", &format!("/v2/assets/{asset}/owners"));
    let mut owners = Vec::new();
    let mut cursor = String::new();
    for _ in 0..max_pages.max(1) {
        let query = [
            ("limit", "100".to_string()),
            ("sortOrder", "Asc".to_string()),
            ("cursor", cursor.clone()),
        ];
        let value = client.get_query(&path, &query).await?;
        for entry in first_array(&value, &["data"]) {
            let Some(owner) = entry.get("owner").filter(|owner| owner.is_object()) else { continue };
            if !str_at(owner, "type").eq_ignore_ascii_case("user") && !str_at(owner, "type").is_empty() {
                continue;
            }
            let user_id = num_at(owner, "id");
            if user_id == 0 {
                continue;
            }
            owners.push(AssetOwner {
                user_asset_id: num_at(entry, "id"),
                user_id,
                name: str_at(owner, "name"),
                serial: opt_num_at(entry, "serialNumber").filter(|serial| *serial > 0),
                updated: str_at(entry, "updated"),
            });
        }
        match value.get("nextPageCursor").and_then(Value::as_str) {
            Some(next) if !next.is_empty() => cursor = next.to_string(),
            _ => break,
        }
    }
    Ok(owners)
}

/// A trade's offers without thumbnails (for statistics over many trades).
pub async fn trade_offers(client: &ApiClient, id: u64) -> Result<TradeDetail> {
    let value = client.get(&api("trades", &format!("/v1/trades/{id}"))).await?;
    Ok(TradeDetail::from_value(&value))
}

/// All of a player's collectibles (up to 5 pages of 100), without thumbnails.
pub async fn collectibles_all(client: &ApiClient, id: u64) -> Result<Vec<Collectible>> {
    let path = api("inventory", &format!("/v1/users/{id}/assets/collectibles"));
    let mut items = Vec::new();
    let mut cursor = String::new();
    for _ in 0..5 {
        let value = client
            .get_query(&path, &[("limit", "100".to_string()), ("cursor", cursor.clone())])
            .await?;
        items.extend(
            first_array(&value, &["data"])
                .iter()
                .map(Collectible::from_value)
                .filter(|item| item.id != 0 && item.user_asset_id != 0),
        );
        match value.get("nextPageCursor").and_then(Value::as_str) {
            Some(next) if !next.is_empty() => cursor = next.to_string(),
            _ => break,
        }
    }
    Ok(items)
}
