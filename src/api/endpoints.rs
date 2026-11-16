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
    Ok(TransactionsPage {
        items: first_array(&value, &["data"])
            .iter()
            .map(Transaction::from_value)
            .collect(),
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
    Ok(MessagesPage { items, total })
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
    if let Ok(states) = presence(client, &ids).await {
        for friend in &mut friends {
            if let Some(state) = states.get(&friend.id) {
                friend.presence = *state;
                friend.is_online = *state > 0;
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

pub async fn presence(client: &ApiClient, ids: &[u64]) -> Result<HashMap<u64, i64>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let body = json!({ "userIds": ids });
    let value = client
        .post(&api("presence", "/v1/presence/users"), Some(body))
        .await?;
    let mut presence = HashMap::new();
    for entry in first_array(&value, &["userPresences"]) {
        presence.insert(num_at(entry, "userId"), presence_number_of(entry));
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

    Ok(GroupDetail {
        info,
        roles,
        wall,
        icon,
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
