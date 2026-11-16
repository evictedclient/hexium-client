//! Tolerant models over the documented `__NEXT_DATA__` + captured response shapes.
//!
//! The site proxies Roblox's namespaces, and the capture in `docs/hexium.md` only
//! pins down part of the surface. Everything here therefore parses defensively:
//! unknown fields are ignored, numbers may arrive as strings, and each bundle
//! (profile, games, catalog…) is assembled from whichever calls actually answered.

use std::collections::HashMap;

use serde_json::Value;

pub fn str_at(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

pub fn num_at(value: &Value, key: &str) -> u64 {
    match value.get(key) {
        Some(Value::Number(number)) => number.as_u64().unwrap_or(0),
        Some(Value::String(text)) => text.parse().unwrap_or(0),
        _ => 0,
    }
}

pub fn opt_num_at(value: &Value, key: &str) -> Option<u64> {
    match value.get(key) {
        Some(Value::Number(number)) => number.as_u64(),
        Some(Value::String(text)) => text.parse().ok(),
        _ => None,
    }
}

pub fn bool_at(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

pub fn arr_at<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// First array found under any of `keys`, or the root array itself.
pub fn first_array<'a>(value: &'a Value, keys: &[&str]) -> &'a [Value] {
    for key in keys {
        if let Some(items) = value.get(*key).and_then(Value::as_array) {
            return items.as_slice();
        }
    }
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}

pub fn flat_object(value: Option<&Value>) -> Vec<(String, String)> {
    let Some(Value::Object(map)) = value else {
        return Vec::new();
    };
    map.iter()
        .filter_map(|(key, value)| match value {
            Value::Number(number) => Some((key.clone(), number.to_string())),
            Value::String(text) => Some((key.clone(), text.clone())),
            Value::Bool(flag) => Some((key.clone(), flag.to_string())),
            _ => None,
        })
        .collect()
}

fn optional_text(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

/// Roblox's numeric asset type ids, as the catalog details endpoint returns them.
pub fn asset_type_name(id: u64) -> Option<&'static str> {
    Some(match id {
        1 => "Image",
        2 => "T-Shirt",
        3 => "Audio",
        4 => "Mesh",
        5 => "Lua",
        8 => "Hat",
        9 => "Place",
        10 => "Model",
        11 => "Shirt",
        12 => "Pants",
        13 => "Decal",
        17 => "Head",
        18 => "Face",
        19 => "Gear",
        21 => "Badge",
        24 => "Animation",
        27 => "Torso",
        28 => "Right Arm",
        29 => "Left Arm",
        30 => "Left Leg",
        31 => "Right Leg",
        32 => "Package",
        34 => "Game Pass",
        38 => "Plugin",
        40 => "Mesh Part",
        41 => "Hair Accessory",
        42 => "Face Accessory",
        43 => "Neck Accessory",
        44 => "Shoulder Accessory",
        45 => "Front Accessory",
        46 => "Back Accessory",
        47 => "Waist Accessory",
        48 => "Climb Animation",
        49 => "Death Animation",
        50 => "Fall Animation",
        51 => "Idle Animation",
        52 => "Jump Animation",
        53 => "Run Animation",
        54 => "Swim Animation",
        55 => "Walk Animation",
        56 => "Pose Animation",
        61 => "Emote Animation",
        62 => "Video",
        _ => return None,
    })
}

/// `assetType` arrives as a number (catalog), a string (avatar) or `{ id, name }`.
pub fn asset_type_of(value: &Value) -> String {
    let by_id = |id: Option<u64>| match id {
        Some(id) => asset_type_name(id)
            .map(str::to_string)
            .unwrap_or_else(|| format!("Type {id}")),
        None => String::new(),
    };
    match value.get("assetType") {
        Some(Value::String(text)) => match text.parse::<u64>() {
            Ok(id) => by_id(Some(id)),
            Err(_) => text.clone(),
        },
        Some(Value::Number(number)) => by_id(number.as_u64()),
        Some(object @ Value::Object(_)) => {
            let name = str_at(object, "name");
            if name.is_empty() {
                by_id(opt_num_at(object, "id"))
            } else {
                name
            }
        }
        _ => String::new(),
    }
}

/// `/catalog/{id}/{slug}` and `/games/{id}/{slug}` want a lowercase slug.
pub fn slug(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut dash = false;
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            out.push(character.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

#[derive(Debug, Clone, Default)]
pub struct AuthUser {
    pub id: u64,
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub is_staff: bool,
    pub is_admin: bool,
    pub is_verified: bool,
    pub has_verified_badge: bool,
    pub has_admin_badge: bool,
}

impl AuthUser {
    pub fn from_value(value: &Value) -> Self {
        let name = str_at(value, "name");
        let display_name = optional_text(value, &["displayName"]).unwrap_or_else(|| name.clone());
        Self {
            id: num_at(value, "id"),
            name,
            display_name,
            description: str_at(value, "description"),
            is_staff: bool_at(value, "isStaff") || bool_at(value, "hasStaffBadge"),
            is_admin: bool_at(value, "isAdmin") || bool_at(value, "hasAdminBadge"),
            is_verified: bool_at(value, "isVerified") || bool_at(value, "hasVerifiedBadge"),
            has_verified_badge: bool_at(value, "hasVerifiedBadge"),
            has_admin_badge: bool_at(value, "hasAdminBadge"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Currency {
    pub robux: i64,
    pub tix: i64,
}

impl Currency {
    pub fn from_value(value: &Value) -> Self {
        let robux = opt_num_at(value, "robux")
            .or_else(|| opt_num_at(value, "robuxBalance"))
            .unwrap_or(0);
        let tix = opt_num_at(value, "tix")
            .or_else(|| opt_num_at(value, "tickets"))
            .or_else(|| opt_num_at(value, "ticketBalance"))
            .unwrap_or(0);
        Self {
            robux: robux as i64,
            tix: tix as i64,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Counts {
    pub unread: u64,
    pub friend_requests: u64,
    pub trades: u64,
}

#[derive(Debug, Clone, Default)]
pub struct Session {
    pub user: AuthUser,
    pub currency: Currency,
    pub counts: Counts,
    pub headshot: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct UserProfile {
    pub id: u64,
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub created: String,
    pub is_banned: bool,
    pub post_count: u64,
    pub is_verified: bool,
    pub has_verified_badge: bool,
    pub has_admin_badge: bool,
}

impl UserProfile {
    pub fn from_value(value: &Value) -> Self {
        let name = str_at(value, "name");
        let display_name = optional_text(value, &["displayName"]).unwrap_or_else(|| name.clone());
        Self {
            id: num_at(value, "id"),
            name,
            display_name,
            description: str_at(value, "description"),
            created: str_at(value, "created"),
            is_banned: bool_at(value, "isBanned"),
            post_count: num_at(value, "postCount"),
            is_verified: bool_at(value, "isVerified"),
            has_verified_badge: bool_at(value, "hasVerifiedBadge"),
            has_admin_badge: bool_at(value, "hasAdminBadge"),
        }
    }
}

fn presence_number(value: &Value) -> i64 {
    match value.get("presenceType") {
        Some(Value::Number(number)) => number.as_i64().unwrap_or(0),
        Some(Value::String(text)) => match text.to_ascii_lowercase().as_str() {
            "online" => 1,
            "ingame" | "in-game" | "ingames" => 2,
            "instudio" | "in-studio" => 3,
            other => other.parse().unwrap_or(0),
        },
        _ => 0,
    }
}

/// The friends list often says `isOnline` while leaving `presenceType` at 0, so the
/// flag decides between "Online" and "Offline" when there is no finer state.
pub fn presence_label(presence: i64, is_online: bool) -> &'static str {
    match presence {
        1 => "Online",
        0 if is_online => "Online",
        2 => "In game",
        3 => "In Studio",
        _ => "Offline",
    }
}

#[derive(Debug, Clone, Default)]
pub struct Friend {
    pub id: u64,
    pub name: String,
    pub display_name: String,
    pub is_online: bool,
    pub presence: i64,
    pub location: String,
}

impl Friend {
    pub fn from_value(value: &Value) -> Self {
        let presence = presence_number(value);
        let name = str_at(value, "name");
        let display_name = optional_text(value, &["displayName"]).unwrap_or_else(|| name.clone());
        Self {
            id: num_at(value, "id"),
            name,
            display_name,
            is_online: bool_at(value, "isOnline") || presence > 0,
            presence,
            location: str_at(value, "lastLocation"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Game {
    pub id: u64,
    pub place_id: Option<u64>,
    pub name: String,
    pub creator: String,
    pub playing: u64,
    pub visits: u64,
    pub votes_up: u64,
    pub votes_down: u64,
    pub description: String,
    pub icon_url: Option<String>,
}

impl Game {
    pub fn from_value(value: &Value) -> Self {
        let place_id = opt_num_at(value, "placeId")
            .or_else(|| opt_num_at(value, "rootPlaceId"))
            .or_else(|| {
                value
                    .get("rootPlace")
                    .and_then(|root| root.get("id"))
                    .and_then(Value::as_u64)
            });
        let id = opt_num_at(value, "universeId")
            .or_else(|| {
                value
                    .get("universe")
                    .and_then(|universe| universe.get("id"))
                    .and_then(Value::as_u64)
            })
            .or_else(|| opt_num_at(value, "id"))
            .unwrap_or(0);
        let creator = value
            .get("creator")
            .and_then(|creator| creator.get("name"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| str_at(value, "creatorName"));
        Self {
            id,
            place_id,
            name: str_at(value, "name"),
            creator,
            playing: num_at(value, "playerCount").max(num_at(value, "playing")),
            visits: num_at(value, "placeVisits")
                .max(num_at(value, "visits"))
                .max(num_at(value, "visitCount")),
            votes_up: num_at(value, "votesUp").max(num_at(value, "totalUpVotes")),
            votes_down: num_at(value, "votesDown").max(num_at(value, "totalDownVotes")),
            description: optional_text(value, &["description", "gameDescription"])
                .unwrap_or_default(),
            icon_url: optional_text(value, &["thumbUrl", "imageUrl", "thumbnailUrl", "iconUrl"]),
        }
    }

    /// Route shape used by the site: `/games/[assetId]/[name]`.
    pub fn site_path(&self) -> String {
        let id = self.place_id.unwrap_or(self.id);
        format!("/games/{}/{}", id, slug(&self.name))
    }
}

#[derive(Debug, Clone, Default)]
pub struct CatalogItem {
    pub id: u64,
    pub item_type: String,
    pub name: String,
    pub price: Option<i64>,
    pub price_tix: Option<i64>,
    pub creator: String,
    pub asset_type: String,
    pub off_sale: bool,
    pub description: String,
}

impl CatalogItem {
    pub fn from_value(value: &Value) -> Self {
        let price = opt_num_at(value, "price")
            .or_else(|| opt_num_at(value, "lowestPrice"))
            .or_else(|| opt_num_at(value, "lowestResalePrice"))
            .map(|price| price as i64);
        let asset_type = asset_type_of(value);
        let off_sale = bool_at(value, "isOffSale")
            || str_at(value, "priceStatus").eq_ignore_ascii_case("off sale")
            || value.get("isForSale").and_then(Value::as_bool) == Some(false);
        Self {
            id: opt_num_at(value, "id")
                .or_else(|| opt_num_at(value, "assetId"))
                .unwrap_or(0),
            item_type: str_at(value, "itemType"),
            name: str_at(value, "name"),
            price,
            price_tix: opt_num_at(value, "priceTickets").map(|price| price as i64),
            creator: str_at(value, "creatorName"),
            asset_type,
            off_sale,
            description: str_at(value, "description"),
        }
    }

    pub fn site_path(&self) -> String {
        format!("/catalog/{}/{}", self.id, slug(&self.name))
    }
}

#[derive(Debug, Clone, Default)]
pub struct UserGroup {
    pub id: u64,
    pub name: String,
    pub role: String,
    pub member_count: u64,
    pub is_primary: bool,
}

impl UserGroup {
    pub fn from_value(value: &Value) -> Self {
        let group = value.get("group").cloned().unwrap_or(Value::Null);
        let role = value.get("role").cloned().unwrap_or(Value::Null);
        Self {
            id: num_at(&group, "id").max(num_at(value, "id")),
            name: str_at(&group, "name"),
            role: str_at(&role, "name"),
            member_count: num_at(&group, "memberCount"),
            is_primary: bool_at(value, "isPrimary"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct GroupInfo {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub member_count: u64,
    pub owner: String,
    pub is_verified: bool,
}

impl GroupInfo {
    pub fn from_value(value: &Value) -> Self {
        let owner = value
            .get("owner")
            .and_then(|owner| owner.get("name"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| str_at(value, "ownerName"));
        Self {
            id: num_at(value, "id"),
            name: str_at(value, "name"),
            description: str_at(value, "description"),
            member_count: num_at(value, "memberCount"),
            owner,
            is_verified: bool_at(value, "hasVerifiedBadge"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct WallPost {
    pub id: u64,
    pub body: String,
    pub poster: String,
    pub created: String,
}

impl WallPost {
    pub fn from_value(value: &Value) -> Self {
        let poster = value
            .get("poster")
            .and_then(|poster| poster.get("name"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| str_at(value, "posterName"));
        Self {
            id: num_at(value, "id"),
            body: str_at(value, "body").replace("<br>", "\n"),
            poster,
            created: str_at(value, "created"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Badge {
    pub id: u64,
    pub name: String,
    pub description: String,
}

impl Badge {
    pub fn from_value(value: &Value) -> Self {
        Self {
            id: num_at(value, "id"),
            name: str_at(value, "name"),
            description: str_at(value, "description"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct WornAsset {
    pub id: u64,
    pub name: String,
    pub asset_type: String,
}

impl WornAsset {
    pub fn from_value(value: &Value) -> Self {
        let asset_type = asset_type_of(value);
        Self {
            id: num_at(value, "id"),
            name: str_at(value, "name"),
            asset_type,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AvatarData {
    pub avatar_type: String,
    pub assets: Vec<WornAsset>,
    pub scales: Vec<(String, String)>,
    pub body_colors: Vec<(String, String)>,
}

impl AvatarData {
    pub fn from_value(value: &Value) -> Self {
        Self {
            avatar_type: str_at(value, "playerAvatarType"),
            assets: first_array(value, &["assets"])
                .iter()
                .map(WornAsset::from_value)
                .collect(),
            scales: flat_object(value.get("scales")),
            body_colors: flat_object(value.get("bodyColors")),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Collectible {
    pub id: u64,
    /// This copy. Resale prices are set per copy, not per item.
    pub user_asset_id: u64,
    pub name: String,
    pub asset_type: String,
    pub serial: Option<u64>,
    /// Recent average price.
    pub rap: u64,
}

impl Collectible {
    pub fn from_value(value: &Value) -> Self {
        Self {
            id: opt_num_at(value, "assetId")
                .or_else(|| opt_num_at(value, "id"))
                .unwrap_or(0),
            user_asset_id: num_at(value, "userAssetId"),
            name: str_at(value, "name"),
            // Despite its name, the collectibles list's `assetTypeId` holds the type
            // name ("Hat", "HairAccessory"); a number is mapped just in case.
            asset_type: {
                let named = asset_type_of(value);
                if !named.is_empty() {
                    named
                } else {
                    let raw = str_at(value, "assetTypeId");
                    match raw.parse::<u64>() {
                        Ok(id) => asset_type_name(id).map(str::to_string).unwrap_or_default(),
                        Err(_) => spaced_words(&raw),
                    }
                }
            },
            serial: opt_num_at(value, "serialNumber"),
            rap: num_at(value, "recentAveragePrice"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct EventItem {
    pub title: String,
    pub description: String,
    pub url: Option<String>,
}

impl EventItem {
    pub fn from_value(value: &Value) -> Self {
        Self {
            title: optional_text(value, &["title", "name", "heading"]).unwrap_or_default(),
            description: optional_text(value, &["description", "text", "body"]).unwrap_or_default(),
            url: optional_text(value, &["url", "link", "href"]),
        }
    }
}

// ---------------------------------------------------------------------------
// Bundles: one message per screen instead of one per request.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct FriendsBundle {
    pub friends: Vec<Friend>,
    pub icons: HashMap<u64, String>,
}

#[derive(Debug, Clone, Default)]
pub struct GamesBundle {
    pub games: Vec<Game>,
    pub icons: HashMap<u64, String>,
}

#[derive(Debug, Clone, Default)]
pub struct CatalogBundle {
    pub items: Vec<CatalogItem>,
    pub icons: HashMap<u64, String>,
    /// Cursor for the next page, when the search endpoint offered one.
    pub next: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct AvatarBundle {
    pub data: AvatarData,
    pub icons: HashMap<u64, String>,
    pub render: Option<String>,
    /// BrickColor id -> its swatch, from the site's own avatar rules.
    pub palette: HashMap<u64, PaletteColour>,
}

/// One entry of `avatar/v1/avatar-rules` `bodyColorsPalette`.
#[derive(Debug, Clone, Default)]
pub struct PaletteColour {
    /// `#RRGGBB`.
    pub hex: String,
    pub name: String,
}

#[derive(Debug, Clone, Default)]
pub struct GroupsBundle {
    pub groups: Vec<UserGroup>,
    pub icons: HashMap<u64, String>,
}

#[derive(Debug, Clone, Default)]
pub struct GroupDetail {
    pub info: GroupInfo,
    pub roles: Vec<(String, u64)>,
    pub wall: Vec<WallPost>,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct InventoryBundle {
    pub owned: Vec<u64>,
    pub collectibles: Vec<Collectible>,
    pub icons: HashMap<u64, String>,
}

#[derive(Debug, Clone, Default)]
pub struct ProfileBundle {
    pub user: UserProfile,
    pub status: String,
    pub followers: u64,
    pub following: u64,
    pub friends: Vec<Friend>,
    pub icons: HashMap<u64, String>,
    pub badges: Vec<Badge>,
    pub groups: Vec<UserGroup>,
    pub group_icons: HashMap<u64, String>,
    pub games: Vec<Game>,
    pub game_icons: HashMap<u64, String>,
    pub avatar: Option<String>,
}

/// One row of `economy/v2/users/{id}/transactions`.
#[derive(Debug, Clone, Default)]
pub struct Transaction {
    pub id: u64,
    pub kind: String,
    pub created: String,
    pub pending: bool,
    pub agent_id: u64,
    pub agent_name: String,
    pub item_id: u64,
    pub item_name: String,
    pub item_kind: String,
    pub amount: i64,
    pub currency: String,
}

impl Transaction {
    pub fn from_value(value: &Value) -> Self {
        let empty = Value::Null;
        let agent = value.get("agent").unwrap_or(&empty);
        let details = value.get("details").unwrap_or(&empty);
        let currency = value.get("currency").unwrap_or(&empty);
        let amount = match currency.get("amount") {
            Some(Value::Number(number)) => number
                .as_i64()
                .or_else(|| number.as_f64().map(|float| float as i64))
                .unwrap_or(0),
            Some(Value::String(text)) => text.parse().unwrap_or(0),
            _ => 0,
        };
        Self {
            id: num_at(value, "id"),
            kind: str_at(value, "transactionType"),
            created: str_at(value, "created"),
            pending: bool_at(value, "isPending"),
            agent_id: num_at(agent, "id"),
            agent_name: str_at(agent, "name"),
            item_id: num_at(details, "id"),
            item_name: str_at(details, "name"),
            item_kind: str_at(details, "type"),
            amount,
            currency: str_at(currency, "type"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TransactionsPage {
    pub items: Vec<Transaction>,
    pub next: Option<String>,
}

/// One private message or announcement from `privatemessages/v1`.
#[derive(Debug, Clone, Default)]
pub struct Message {
    pub id: u64,
    pub sender_id: u64,
    pub sender_name: String,
    pub recipient_name: String,
    pub subject: String,
    pub body: String,
    pub created: String,
    pub is_read: bool,
    pub is_system: bool,
}

impl Message {
    pub fn from_value(value: &Value) -> Self {
        let empty = Value::Null;
        let sender = value.get("sender").unwrap_or(&empty);
        let recipient = value.get("recipient").unwrap_or(&empty);
        Self {
            id: num_at(value, "id"),
            sender_id: num_at(sender, "id"),
            sender_name: optional_text(sender, &["name", "displayName"]).unwrap_or_default(),
            recipient_name: optional_text(recipient, &["name", "displayName"])
                .unwrap_or_default(),
            subject: str_at(value, "subject"),
            body: str_at(value, "body"),
            created: str_at(value, "created"),
            // Announcements carry no read flag; they are not "unread".
            is_read: value.get("isRead").and_then(Value::as_bool).unwrap_or(true),
            is_system: bool_at(value, "isSystemMessage"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct MessagesPage {
    pub items: Vec<Message>,
    /// `totalCollectionSize`. The response's `totalPages` is not trustworthy (it
    /// said 1 for 35 messages at 20 a page), so paging goes by this count.
    pub total: u64,
}

/// One row of `trades/v1/trades/{tab}`. `user` is always the other party.
#[derive(Debug, Clone, Default)]
pub struct TradeSummary {
    pub id: u64,
    pub partner_id: u64,
    pub partner_name: String,
    pub created: String,
    pub expiration: String,
    pub is_active: bool,
    pub status: String,
}

impl TradeSummary {
    pub fn from_value(value: &Value) -> Self {
        let empty = Value::Null;
        let user = value.get("user").unwrap_or(&empty);
        Self {
            id: num_at(value, "id"),
            partner_id: num_at(user, "id"),
            partner_name: optional_text(user, &["name", "displayName"]).unwrap_or_default(),
            created: str_at(value, "created"),
            expiration: str_at(value, "expiration"),
            is_active: bool_at(value, "isActive"),
            status: str_at(value, "status"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TradesPage {
    pub items: Vec<TradeSummary>,
    /// Partner headshots by user id.
    pub icons: HashMap<u64, String>,
    pub next: Option<String>,
}

/// One copy of an item on one side of a trade.
#[derive(Debug, Clone, Default)]
pub struct TradeAsset {
    pub user_asset_id: u64,
    pub asset_id: u64,
    pub name: String,
    pub serial: Option<u64>,
    pub rap: u64,
}

impl TradeAsset {
    pub fn from_value(value: &Value) -> Self {
        Self {
            user_asset_id: num_at(value, "id"),
            asset_id: num_at(value, "assetId"),
            name: str_at(value, "name"),
            serial: opt_num_at(value, "serialNumber"),
            rap: num_at(value, "recentAveragePrice"),
        }
    }
}

/// One side of a trade. `robux` was null on every trade checked.
#[derive(Debug, Clone, Default)]
pub struct TradeOffer {
    pub user_id: u64,
    pub user_name: String,
    pub assets: Vec<TradeAsset>,
    pub robux: Option<u64>,
}

impl TradeOffer {
    pub fn from_value(value: &Value) -> Self {
        let empty = Value::Null;
        let user = value.get("user").unwrap_or(&empty);
        Self {
            user_id: num_at(user, "id"),
            user_name: optional_text(user, &["name", "displayName"]).unwrap_or_default(),
            assets: arr_at(value, "userAssets")
                .iter()
                .map(TradeAsset::from_value)
                .collect(),
            robux: opt_num_at(value, "robux").filter(|robux| *robux > 0),
        }
    }

    pub fn total_rap(&self) -> u64 {
        self.assets.iter().map(|asset| asset.rap).sum()
    }
}

/// `trades/v1/trades/{id}`. The order of `offers` is not fixed (your side came
/// first on outbound and completed trades but second on an inactive one), so
/// sides are told apart by user id, never by position.
#[derive(Debug, Clone, Default)]
pub struct TradeDetail {
    pub id: u64,
    pub status: String,
    pub created: String,
    pub expiration: String,
    pub is_active: bool,
    pub offers: Vec<TradeOffer>,
    /// Item thumbnails by asset id.
    pub icons: HashMap<u64, String>,
}

impl TradeDetail {
    pub fn from_value(value: &Value) -> Self {
        Self {
            id: num_at(value, "id"),
            status: str_at(value, "status"),
            created: str_at(value, "created"),
            expiration: str_at(value, "expiration"),
            is_active: bool_at(value, "isActive"),
            offers: arr_at(value, "offers")
                .iter()
                .map(TradeOffer::from_value)
                .collect(),
            icons: HashMap::new(),
        }
    }
}

/// One pending request from `friends/v1/my/friends/requests`. The item is the
/// sender's user record (`id`, `name`, `displayName`, `created`), which is how the
/// site's own notification code reads it.
#[derive(Debug, Clone, Default)]
pub struct FriendRequest {
    pub user_id: u64,
    pub name: String,
    pub display_name: String,
    pub created: String,
}

impl FriendRequest {
    pub fn from_value(value: &Value) -> Self {
        let name = optional_text(value, &["name", "username"]).unwrap_or_default();
        let display_name = optional_text(value, &["displayName"]).unwrap_or_else(|| name.clone());
        Self {
            user_id: num_at(value, "id"),
            name,
            display_name,
            created: str_at(value, "created"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct FriendRequestsBundle {
    pub items: Vec<FriendRequest>,
    pub icons: HashMap<u64, String>,
}

/// What the Sell window needs about one copy of a collectible.
#[derive(Debug, Clone, Default)]
pub struct ResaleInfo {
    /// Your asking price for this copy; `None` when it is not on sale.
    pub my_price: Option<u64>,
    /// Cheapest copy someone else is selling, if any.
    pub lowest_other: Option<u64>,
    /// How many other copies are on sale.
    pub other_sellers: usize,
    pub sales: u64,
    pub rap: u64,
}

/// `HairAccessory` -> `Hair Accessory`.
pub fn spaced_words(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 4);
    for (index, character) in text.chars().enumerate() {
        if index > 0 && character.is_ascii_uppercase() {
            out.push(' ');
        }
        out.push(character);
    }
    out
}

/// Votes on one game plus whether, and how, you voted.
#[derive(Debug, Clone, Default)]
pub struct GameVoteState {
    pub up: u64,
    pub down: u64,
    /// `Some(true)` thumbs up, `Some(false)` thumbs down, `None` no vote.
    pub user_vote: Option<bool>,
    pub can_vote: bool,
    /// Why voting is not allowed, when the site says.
    pub reason: Option<String>,
}

/// Someone's tradeable collectibles with their thumbnails, for the trade builder.
#[derive(Debug, Clone, Default)]
pub struct TradeInventory {
    /// Highest RAP first.
    pub items: Vec<Collectible>,
    /// Asset id -> thumbnail.
    pub icons: HashMap<u64, String>,
}
