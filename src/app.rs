//! Application state: one screen per message, all network work on a tokio runtime,
//! results drained on the UI thread each frame.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::api::hub::{self, HubEvent, HubHandle, HubStatus};
use crate::api::{endpoints, models, ApiClient};
use crate::config::Config;
use crate::ui::{Ctx, Thumbs};

pub type Res<T> = std::result::Result<T, String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Home,
    Games,
    Catalog,
    Avatar,
    Profile,
    Friends,
    Groups,
    Inventory,
    Money,
    Messages,
    Chat,
    Trades,
    Settings,
}

impl View {
    pub const ALL: [View; 13] = [
        View::Home,
        View::Games,
        View::Catalog,
        View::Avatar,
        View::Profile,
        View::Friends,
        View::Groups,
        View::Inventory,
        View::Money,
        View::Messages,
        View::Chat,
        View::Trades,
        View::Settings,
    ];

    pub fn label(self) -> &'static str {
        match self {
            View::Home => "Home",
            View::Games => "Games",
            View::Catalog => "Catalog",
            View::Avatar => "Avatar",
            View::Profile => "Profile",
            View::Friends => "Friends",
            View::Groups => "Groups",
            View::Inventory => "Inventory",
            View::Money => "Money",
            View::Messages => "Messages",
            View::Chat => "Chat",
            View::Trades => "Trades",
            View::Settings => "Settings",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Load<T> {
    Idle,
    Busy,
    Ready(T),
    Err(String),
}

impl<T> Default for Load<T> {
    fn default() -> Self {
        Load::Idle
    }
}

impl<T> Load<T> {
    pub fn is_idle(&self) -> bool {
        matches!(self, Load::Idle)
    }

    pub fn ready(&self) -> Option<&T> {
        match self {
            Load::Ready(value) => Some(value),
            _ => None,
        }
    }
}

pub enum Msg {
    Session(Res<models::Session>),
    Thumb(String, Option<Arc<[u8]>>),
    Alert(Option<String>),
    Maintenance(Option<String>),
    Events(Res<Vec<models::EventItem>>),
    Friends(Res<models::FriendsBundle>),
    Sorts(Res<Vec<(String, String)>>),
    Games(Res<models::GamesBundle>),
    Nav(Res<Vec<(String, String)>>),
    Catalog(Res<models::CatalogBundle>),
    CatalogMore(Res<models::CatalogBundle>),
    CatalogDetail(Res<models::CatalogItem>),
    Profile(Res<models::ProfileBundle>),
    Avatar(Res<models::AvatarBundle>),
    Groups(Res<models::GroupsBundle>),
    GroupDetail(Res<models::GroupDetail>),
    Inventory(Res<models::InventoryBundle>),
    Transactions(Res<models::TransactionsPage>),
    TransactionsMore(Res<models::TransactionsPage>),
    Messages(Res<models::MessagesPage>),
    MessagesMore(Res<models::MessagesPage>),
    Trades(Res<models::TradesPage>),
    TradesMore(Res<models::TradesPage>),
    TradeDetail(u64, Res<models::TradeDetail>),
    Probe(bool),
    FollowState(u64, Res<bool>),
    FriendshipState(u64, Res<String>),
    FriendRequests(Res<models::FriendRequestsBundle>),
    UnreadIds(Res<Vec<u64>>),
    TradeInventory { owner: u64, result: Res<models::TradeInventory> },
    GameVote(u64, Res<models::GameVoteState>),
    Resale(u64, Res<models::ResaleInfo>),
    FavouriteState(u64, Res<bool>),
    WriteDone(WriteAction, Res<()>),
    ChatList(Res<models::ChatBundle>),
    ChatMessages(u64, Res<Vec<models::ChatMessage>>),
    /// A chat message went out: the conversation it went to (new or existing)
    /// and the message as the site echoed it.
    ChatSent(models::ChatConversation, models::ChatMessage),
    Hub(HubEvent),
    /// A chat notification was clicked.
    OpenChat(u64),
    /// Play finished: the place and the client year, or why it failed.
    GameLaunched(u64, Res<String>),
    /// Join friend finished: the friend, and the client year or why not.
    FriendJoined(u64, Res<String>),
    /// A page of servers: (place, first index asked for, result).
    GameServers(u64, u64, Res<models::ServerPage>),
    /// A trades / friend requests / messages notification was clicked.
    OpenView(View),
    /// The watcher saw a counter move: refresh the badges.
    ActivityChanged,
}

/// Something that changes the signed-in account. Nothing here is sent until the
/// user confirms it in the dialog.
#[derive(Debug, Clone)]
pub enum WriteAction {
    Follow { user_id: u64, name: String, follow: bool },
    Favourite { asset_id: u64, name: String, favourite: bool },
    SetStatus { status: String },
    MarkRead { message_id: u64, subject: String, read: bool },
    Archive { message_id: u64, subject: String, archive: bool },
    MarkAllRead { ids: Vec<u64> },
    VoteGame { universe_id: u64, name: String, up: bool, previous: Option<bool> },
    /// A new trade, or a counter when `counter_of` is set.
    SendTrade {
        partner_id: u64,
        partner: String,
        counter_of: Option<u64>,
        give_ids: Vec<u64>,
        get_ids: Vec<u64>,
        give: TradeSide,
        get: TradeSide,
    },
    SetDescription { description: String },
    RequestFriend { user_id: u64, name: String },
    AcceptFriend { user_id: u64, name: String },
    DeclineFriend { user_id: u64, name: String },
    Unfriend { user_id: u64, name: String },
    /// List a copy for sale (`old` None), change its price, or take it off sale
    /// (`new` None).
    SetResalePrice {
        asset_id: u64,
        user_asset_id: u64,
        name: String,
        serial: Option<u64>,
        rap: u64,
        lowest_other: Option<u64>,
        old: Option<u64>,
        new: Option<u64>,
    },
    AnswerTrade {
        trade_id: u64,
        partner: String,
        accept: bool,
        give: TradeSide,
        get: TradeSide,
    },
    SendMessage {
        recipient_id: u64,
        recipient: String,
        subject: String,
        body: String,
        reply_to: Option<u64>,
        include_previous: bool,
    },
    /// A chat message. With no conversation yet, one is started with `user_id` first.
    SendChat {
        conversation_id: Option<u64>,
        user_id: u64,
        recipient: String,
        text: String,
    },
}

impl WriteAction {
    pub fn title(&self) -> String {
        match self {
            WriteAction::Follow { follow: true, .. } => "Follow user".to_string(),
            WriteAction::Follow { follow: false, .. } => "Unfollow user".to_string(),
            WriteAction::Favourite { favourite: true, .. } => "Add to favourites".to_string(),
            WriteAction::Favourite { favourite: false, .. } => "Remove from favourites".to_string(),
            WriteAction::SetStatus { .. } => "Change status".to_string(),
            WriteAction::MarkRead { read: true, .. } => "Mark as read".to_string(),
            WriteAction::MarkRead { read: false, .. } => "Mark as unread".to_string(),
            WriteAction::Archive { archive: true, .. } => "Archive message".to_string(),
            WriteAction::Archive { archive: false, .. } => "Move to inbox".to_string(),
            WriteAction::MarkAllRead { .. } => "Mark all as read".to_string(),
            WriteAction::SendTrade { counter_of: Some(_), .. } => "Send counter offer".to_string(),
            WriteAction::SendTrade { .. } => "Send trade".to_string(),
            WriteAction::VoteGame { up: true, .. } => "Thumbs up".to_string(),
            WriteAction::VoteGame { up: false, .. } => "Thumbs down".to_string(),
            WriteAction::SetDescription { .. } => "Change description".to_string(),
            WriteAction::RequestFriend { .. } => "Send friend request".to_string(),
            WriteAction::AcceptFriend { .. } => "Accept friend request".to_string(),
            WriteAction::DeclineFriend { .. } => "Decline friend request".to_string(),
            WriteAction::Unfriend { .. } => "Unfriend".to_string(),
            WriteAction::SetResalePrice { new: None, .. } => "Take off sale".to_string(),
            WriteAction::SetResalePrice { old: None, .. } => "List for sale".to_string(),
            WriteAction::SetResalePrice { .. } => "Change price".to_string(),
            WriteAction::AnswerTrade { accept: true, .. } => "Accept trade".to_string(),
            WriteAction::AnswerTrade { accept: false, .. } => "Decline trade".to_string(),
            WriteAction::SendMessage { reply_to: Some(_), .. } => "Send reply".to_string(),
            WriteAction::SendMessage { .. } => "Send message".to_string(),
            WriteAction::SendChat { conversation_id: None, .. } => "Start chat".to_string(),
            WriteAction::SendChat { .. } => "Send chat message".to_string(),
        }
    }

    /// Exactly what will happen, in plain words.
    pub fn description(&self) -> String {
        match self {
            WriteAction::Follow { name, follow, .. } => {
                if *follow {
                    format!("Your account will follow {name}. They can see this.")
                } else {
                    format!("Your account will stop following {name}.")
                }
            }
            WriteAction::Favourite { name, favourite, .. } => {
                if *favourite {
                    format!("\"{name}\" will be added to your favourites, which are shown on your profile.")
                } else {
                    format!("\"{name}\" will be removed from your favourites.")
                }
            }
            WriteAction::SetStatus { status } => {
                if status.trim().is_empty() {
                    "Your status will be cleared.".to_string()
                } else {
                    format!("Your status will be set to \"{status}\". Anyone who opens your profile can see it.")
                }
            }
            WriteAction::Archive { subject, archive, .. } => {
                if *archive {
                    format!("\"{subject}\" moves to your Archive. You can move it back to the inbox from there.")
                } else {
                    format!("\"{subject}\" moves back to your inbox.")
                }
            }
            WriteAction::SendTrade { partner, counter_of, give, get, .. } => {
                offer_summary(partner, counter_of.is_some(), give, get)
            }
            WriteAction::VoteGame { name, up, previous, .. } => {
                let which = if *up { "thumbs up" } else { "thumbs down" };
                let mut text = format!("Your account gives \"{name}\" a {which}.");
                if *previous == Some(!*up) {
                    let old = if *up { "thumbs down" } else { "thumbs up" };
                    text.push_str(&format!(" This replaces your {old}."));
                }
                text
            }
            WriteAction::SetDescription { description } => {
                if description.trim().is_empty() {
                    "Your profile description will be cleared.".to_string()
                } else {
                    let preview: String = description.chars().take(300).collect();
                    let more = if description.chars().count() > 300 { "…" } else { "" };
                    format!("Your profile description will be set to:\n\n{preview}{more}\n\nAnyone who opens your profile can see it.")
                }
            }
            WriteAction::MarkAllRead { ids } => {
                let count = ids.len();
                let noun = if count == 1 { "message" } else { "messages" };
                format!(
                    "All {count} unread {noun} in your inbox will be marked as read, including ones not loaded on screen. You can mark any of them unread again one at a time."
                )
            }
            WriteAction::MarkRead { subject, read, .. } => {
                let state = if *read { "read" } else { "unread" };
                format!("The message \"{subject}\" will be marked as {state}.")
            }
            WriteAction::RequestFriend { name, .. } => {
                format!("A friend request will be sent to {name}. They will be notified.")
            }
            WriteAction::AcceptFriend { name, .. } => format!("{name} will be added to your friends."),
            WriteAction::DeclineFriend { name, .. } => {
                format!("{name}'s friend request will be declined.")
            }
            WriteAction::Unfriend { name, .. } => format!(
                "{name} will be removed from your friends. Undoing this takes a new request that they have to accept."
            ),
            WriteAction::SetResalePrice { name, serial, rap, lowest_other, old, new, .. } => {
                resale_summary(name, *serial, *rap, *lowest_other, *old, *new)
            }
            WriteAction::AnswerTrade { partner, accept, give, get, .. } => {
                trade_summary(partner, *accept, give, get)
            }
            WriteAction::SendChat { conversation_id, recipient, text, .. } => {
                chat_summary(recipient, conversation_id.is_none(), text)
            }
            WriteAction::SendMessage { recipient, subject, body, reply_to, include_previous, .. } => {
                let preview: String = body.chars().take(160).collect();
                let more = if body.chars().count() > 160 { "…" } else { "" };
                let quoting = if reply_to.is_some() && *include_previous {
                    " Their earlier message will be quoted under yours."
                } else {
                    ""
                };
                format!(
                    "A private message will be sent to {recipient}.\n\nSubject: {subject}\n{preview}{more}\n\nIt cannot be unsent.{quoting}"
                )
            }
        }
    }

    pub fn confirm_label(&self) -> &'static str {
        match self {
            WriteAction::Follow { follow: true, .. } => "Follow",
            WriteAction::Follow { follow: false, .. } => "Unfollow",
            WriteAction::Favourite { favourite: true, .. } => "Favourite",
            WriteAction::Favourite { favourite: false, .. } => "Remove",
            WriteAction::SetStatus { .. } => "Save status",
            WriteAction::MarkRead { read: true, .. } => "Mark read",
            WriteAction::MarkRead { read: false, .. } => "Mark unread",
            WriteAction::Archive { archive: true, .. } => "Archive",
            WriteAction::Archive { archive: false, .. } => "Move to inbox",
            WriteAction::MarkAllRead { .. } => "Mark all read",
            WriteAction::SendTrade { counter_of: Some(_), .. } => "Send counter",
            WriteAction::SendTrade { .. } => "Send trade",
            WriteAction::VoteGame { up: true, .. } => "Thumbs up",
            WriteAction::VoteGame { up: false, .. } => "Thumbs down",
            WriteAction::SetDescription { .. } => "Save description",
            WriteAction::RequestFriend { .. } => "Send request",
            WriteAction::AcceptFriend { .. } => "Accept",
            WriteAction::DeclineFriend { .. } => "Decline",
            WriteAction::Unfriend { .. } => "Unfriend",
            WriteAction::SetResalePrice { new: None, .. } => "Take off sale",
            WriteAction::SetResalePrice { old: None, .. } => "List for sale",
            WriteAction::SetResalePrice { .. } => "Change price",
            WriteAction::AnswerTrade { accept: true, .. } => "Accept trade",
            WriteAction::AnswerTrade { accept: false, .. } => "Decline trade",
            WriteAction::SendMessage { .. } => "Send",
            WriteAction::SendChat { .. } => "Send",
        }
    }
}

/// One side of a trade, as the confirmation dialog shows it.
#[derive(Debug, Clone, Default)]
pub struct TradeSide {
    /// (name, RAP, serial)
    pub items: Vec<(String, u64, Option<u64>)>,
    pub rap: u64,
    pub robux: Option<u64>,
}

impl From<&models::TradeOffer> for TradeSide {
    fn from(offer: &models::TradeOffer) -> Self {
        Self {
            items: offer
                .assets
                .iter()
                .map(|asset| (asset.name.clone(), asset.rap, asset.serial))
                .collect(),
            rap: offer.total_rap(),
            robux: offer.robux,
        }
    }
}

/// Everything the user should see before answering a trade: both sides item by
/// item, their RAP totals, and which way the difference goes.
fn trade_summary(partner: &str, accept: bool, give: &TradeSide, get: &TradeSide) -> String {
    fn side(out: &mut String, heading: &str, side: &TradeSide) {
        out.push_str(&format!("{heading} (RAP {})\n", group_digits(side.rap)));
        if side.items.is_empty() {
            out.push_str("   no items\n");
        }
        for (name, rap, serial) in &side.items {
            let serial = serial.map(|serial| format!("  #{serial}")).unwrap_or_default();
            out.push_str(&format!("   · {name}  (RAP {}){serial}\n", group_digits(*rap)));
        }
        if let Some(robux) = side.robux {
            out.push_str(&format!("   · plus {} R$\n", group_digits(robux)));
        }
    }

    let mut out = if accept {
        format!("Accept {partner}'s trade. The items move straight away and it cannot be undone.\n\n")
    } else {
        format!("Decline {partner}'s trade. It cannot be reopened; they would have to send a new one.\n\n")
    };
    side(&mut out, if accept { "You give" } else { "You would have given" }, give);
    out.push('\n');
    side(&mut out, if accept { "You get" } else { "You would have got" }, get);
    out.push('\n');
    out.push_str(&match get.rap.cmp(&give.rap) {
        std::cmp::Ordering::Greater => {
            format!("You get {} RAP more than you give.", group_digits(get.rap - give.rap))
        }
        std::cmp::Ordering::Less => {
            format!("You give {} RAP more than you get.", group_digits(give.rap - get.rap))
        }
        std::cmp::Ordering::Equal => "Both sides are worth the same RAP.".to_string(),
    });
    out
}

/// What listing, repricing or delisting a copy will do, with the numbers that
/// matter for the decision.
fn resale_summary(
    name: &str,
    serial: Option<u64>,
    rap: u64,
    lowest_other: Option<u64>,
    old: Option<u64>,
    new: Option<u64>,
) -> String {
    let item = match serial {
        Some(serial) => format!("\"{name}\" #{serial}"),
        None => format!("\"{name}\""),
    };
    let r = |value: u64| format!("{} R$", group_digits(value));
    let mut out = match (old, new) {
        (_, None) => {
            return format!("{item} comes off sale. Nobody can buy it until you list it again.");
        }
        (None, Some(price)) => format!("{item} goes on sale for {}.", r(price)),
        (Some(old), Some(price)) => format!("{item}'s price changes from {} to {}.", r(old), r(price)),
    };
    let price = new.unwrap_or(0);
    out.push_str(" Anyone can buy it at that price straight away, and it leaves your inventory when they do.\n\n");
    out.push_str(&format!("RAP: {}", r(rap)));
    if rap > 0 && price < rap {
        let below = (rap - price) * 100 / rap;
        out.push_str(&format!(" (your price is {below}% below it)"));
    }
    out.push('\n');
    match lowest_other {
        Some(lowest) if price > lowest => {
            out.push_str(&format!("Cheapest on sale now: {} (yours would cost more)", r(lowest)))
        }
        Some(lowest) => out.push_str(&format!("Cheapest on sale now: {}", r(lowest))),
        None => out.push_str("Nobody else is selling one right now."),
    }
    out
}

/// What sending (or countering with) this offer means, with both sides listed.
fn offer_summary(partner: &str, counter: bool, give: &TradeSide, get: &TradeSide) -> String {
    let mut out = if counter {
        format!("Send {partner} this counter offer. It replaces their offer, which you can no longer accept.\n\n")
    } else {
        format!("Send {partner} this trade.\n\n")
    };
    for (heading, side) in [("You give", give), ("You get", get)] {
        out.push_str(&format!("{heading} (RAP {})\n", group_digits(side.rap)));
        for (name, rap, serial) in &side.items {
            let serial = serial.map(|serial| format!("  #{serial}")).unwrap_or_default();
            out.push_str(&format!("   · {name}  (RAP {}){serial}\n", group_digits(*rap)));
        }
        out.push('\n');
    }
    out.push_str(&match get.rap.cmp(&give.rap) {
        std::cmp::Ordering::Greater => {
            format!("You would get {} RAP more than you give.", group_digits(get.rap - give.rap))
        }
        std::cmp::Ordering::Less => {
            format!("You would give {} RAP more than you get.", group_digits(give.rap - get.rap))
        }
        std::cmp::Ordering::Equal => "Both sides are worth the same RAP.".to_string(),
    });
    out.push_str("\nNothing moves until they accept, and they can decline or counter.");
    out
}

fn chat_summary(recipient: &str, starts: bool, text: &str) -> String {
    let preview: String = text.chars().take(300).collect();
    let more = if text.chars().count() > 300 { "…" } else { "" };
    let start = if starts {
        format!("This starts a chat with {recipient} and sends:")
    } else {
        format!("This chat message goes to {recipient}:")
    };
    format!("{start}\n\n{preview}{more}\n\nIt cannot be unsent.")
}

fn group_digits(value: u64) -> String {
    crate::ui::money(value as i64)
}

/// Most items either side of a trade can hold (the most seen on the site, and
/// Roblox's own limit).
pub const TRADE_SIDE_LIMIT: usize = 4;

/// The trade builder window: a new trade, or a counter to an inbound one.
pub struct TradeBuilder {
    pub partner_id: u64,
    pub partner: String,
    /// Set when this counters an inbound trade.
    pub counter_of: Option<u64>,
    pub mine: Load<models::TradeInventory>,
    pub theirs: Load<models::TradeInventory>,
    /// Picked copies (user asset ids).
    pub give: Vec<u64>,
    pub get: Vec<u64>,
    pub filter_mine: String,
    pub filter_theirs: String,
}

/// The message being written. It only becomes a write once the user presses
/// Send and then confirms.
#[derive(Debug, Clone, Default)]
pub struct Compose {
    pub recipient_id: u64,
    pub recipient: String,
    pub subject: String,
    pub body: String,
    /// The received message this answers, if it is a reply.
    pub reply_to: Option<u64>,
    pub include_previous: bool,
}

/// The open confirmation dialog, if any.
pub struct PendingWrite {
    pub action: WriteAction,
    pub sending: bool,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct HomeState {
    pub events: Load<Vec<models::EventItem>>,
}

#[derive(Default)]
pub struct FriendsState {
    pub data: Load<models::FriendsBundle>,
    /// Pending requests sent to you.
    pub requests: Load<models::FriendRequestsBundle>,
    /// The last Join friend: (friend, how it went).
    pub join: Option<(u64, Load<String>)>,
}

#[derive(Default)]
pub struct GamesState {
    pub sorts: Load<Vec<(String, String)>>,
    pub sort: String,
    pub keyword: String,
    pub games: Load<models::GamesBundle>,
    pub selected: Option<usize>,
    /// Votes for the game in the details window.
    pub vote: Load<models::GameVoteState>,
    /// The last Play: (place id, how it went). `Busy` while the ticket is fetched.
    pub launch: Option<(u64, Load<String>)>,
    /// The server the last Play/Join was for (`None` for Play).
    pub launch_job: Option<String>,
    /// Running servers of the game in the details window.
    pub servers: Load<models::ServerPage>,
    pub servers_more: bool,
}

#[derive(Default)]
pub struct CatalogState {
    pub nav: Load<Vec<(String, String)>>,
    pub category: String,
    pub keyword: String,
    pub items: Load<models::CatalogBundle>,
    pub cursor: Option<String>,
    pub loading_more: bool,
    pub more_error: Option<String>,
    pub selected: Option<models::CatalogItem>,
    pub detail: Load<models::CatalogItem>,
    /// Favourite state of the item in the details window.
    pub favourite: Load<bool>,
}

#[derive(Default)]
pub struct ProfileState {
    pub user_id: Option<u64>,
    pub data: Load<models::ProfileBundle>,
    /// Whether you follow the user on screen; only loaded for other people.
    pub following: Load<bool>,
    /// The status box on your own profile.
    pub status_input: String,
    /// The description box on your own profile.
    pub description_input: String,
    /// `Friends`, `NotFriends`, `RequestSent` or `RequestReceived`; other people only.
    pub friendship: Load<String>,
}

#[derive(Default)]
pub struct AvatarState {
    pub data: Load<models::AvatarBundle>,
}

#[derive(Default)]
pub struct GroupsState {
    pub data: Load<models::GroupsBundle>,
    pub selected: Option<u64>,
    pub detail: Load<models::GroupDetail>,
}

#[derive(Default)]
pub struct InventoryState {
    pub data: Load<models::InventoryBundle>,
    /// The Sell window, when open.
    pub resale: Option<ResaleState>,
}

/// One collectible copy in the Sell window.
pub struct ResaleState {
    pub asset_id: u64,
    pub user_asset_id: u64,
    pub name: String,
    pub serial: Option<u64>,
    pub rap: u64,
    pub icon: Option<String>,
    pub info: Load<models::ResaleInfo>,
    /// What is typed in the price box.
    pub price_input: String,
}

#[derive(Default)]
pub struct MoneyState {
    pub kind: String,
    pub data: Load<models::TransactionsPage>,
    pub loading_more: bool,
    pub more_error: Option<String>,
}

#[derive(Default)]
pub struct MessagesState {
    pub tab: String,
    pub data: Load<models::MessagesPage>,
    /// Zero-based index of the last page loaded.
    pub page: u32,
    pub loading_more: bool,
    pub more_error: Option<String>,
    /// Collecting unread ids for "Mark all as read".
    pub preparing_all: bool,
}

#[derive(Default)]
pub struct TradesState {
    pub tab: String,
    pub data: Load<models::TradesPage>,
    pub loading_more: bool,
    pub more_error: Option<String>,
    /// Trade whose details window is open.
    pub selected: Option<u64>,
    pub detail: Load<models::TradeDetail>,
}

/// Who the open chat is with: an existing conversation, or a friend you have
/// not chatted with yet (the conversation is started with the first message).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatTarget {
    Conversation(u64),
    NewWith { user_id: u64, name: String },
}

#[derive(Default)]
pub struct ChatState {
    pub list: Load<models::ChatBundle>,
    pub target: Option<ChatTarget>,
    pub messages: Load<Vec<models::ChatMessage>>,
    pub draft: String,
    pub filter: String,
    /// `None` until the hub has said anything.
    pub hub: Option<HubStatus>,
    /// Conversation id -> (user id, typing until, epoch ms).
    pub typing: HashMap<u64, (u64, u64)>,
    /// Last message id marked read per conversation, so it is sent once.
    marked: HashMap<u64, String>,
    polled_at: Option<Instant>,
    listed_at: Option<Instant>,
    /// Bumped when new messages arrive, so the view scrolls to the bottom.
    pub arrivals: u64,
    /// Your own typing broadcast: the conversation, when the last key was
    /// pressed, and when `isTyping` was last sent.
    typing_in: Option<u64>,
    last_key: Option<Instant>,
    typing_sent: Option<Instant>,
}

impl ChatState {
    pub fn unread_count(&self) -> u64 {
        self.list
            .ready()
            .map(|bundle| bundle.conversations.iter().filter(|conversation| conversation.unread).count() as u64)
            .unwrap_or(0)
    }

    pub fn open_conversation(&self) -> Option<u64> {
        match &self.target {
            Some(ChatTarget::Conversation(id)) => Some(*id),
            _ => None,
        }
    }

    pub fn hub_live(&self) -> bool {
        self.hub == Some(HubStatus::Live)
    }
}

pub struct App {
    _runtime: tokio::runtime::Runtime,
    pub rt: tokio::runtime::Handle,
    pub api: Arc<ApiClient>,
    pub tx: Sender<Msg>,
    rx: Receiver<Msg>,
    pub egui: egui::Context,
    pub cfg: Config,
    pub thumbs: Thumbs,
    pub view: View,
    applied_theme: String,
    pub session: Load<models::Session>,
    pub auth_error: Option<String>,
    pub login_input: String,
    pub alert: Option<String>,
    pub maintenance: Option<String>,
    pub stale_error: Option<String>,
    pub probe_result: Option<bool>,
    pub pending_write: Option<PendingWrite>,
    pub compose: Option<Compose>,
    pub trade_builder: Option<TradeBuilder>,
    /// Result of the last write: (succeeded, message).
    pub notice: Option<(bool, String)>,
    pub home: HomeState,
    pub friends: FriendsState,
    pub games: GamesState,
    pub catalog: CatalogState,
    pub profile: ProfileState,
    pub avatar: AvatarState,
    pub groups: GroupsState,
    pub inventory: InventoryState,
    pub money: MoneyState,
    pub messages: MessagesState,
    pub trades: TradesState,
    pub chat: ChatState,
    hub: Option<HubHandle>,
    watch: Option<crate::watch::WatchHandle>,
    notify_gate: crate::notify::SharedGate,
    /// The native window handle, for raising the window from a toast click.
    hwnd: Arc<AtomicIsize>,
    /// Started at sign-in with `--minimized`: minimise on the first frame.
    start_minimized: bool,
    /// The "Open at sign-in" entry, as Windows has it.
    pub autostart: crate::autostart::State,
    /// The notification-area icon, when Windows gave us one.
    tray: Option<Arc<crate::tray::Tray>>,
    /// Set by the tray's Quit, so the close is not turned into a hide.
    quitting: Arc<std::sync::atomic::AtomicBool>,
    refreshed_at: Instant,
    last_resize_check: Instant,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        cfg: Config,
        api: Arc<ApiClient>,
        runtime: tokio::runtime::Runtime,
        start_minimized: bool,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let rt = runtime.handle().clone();
        let mut app = Self {
            _runtime: runtime,
            rt,
            api,
            tx,
            rx,
            egui: cc.egui_ctx.clone(),
            applied_theme: String::new(),
            cfg,
            thumbs: Thumbs::default(),
            view: View::Home,
            session: Load::Idle,
            auth_error: None,
            login_input: String::new(),
            alert: None,
            maintenance: None,
            stale_error: None,
            probe_result: None,
            pending_write: None,
            compose: None,
            trade_builder: None,
            notice: None,
            home: HomeState::default(),
            friends: FriendsState::default(),
            games: GamesState::default(),
            catalog: CatalogState::default(),
            profile: ProfileState::default(),
            avatar: AvatarState::default(),
            groups: GroupsState::default(),
            inventory: InventoryState::default(),
            money: MoneyState::default(),
            messages: MessagesState::default(),
            trades: TradesState::default(),
            chat: ChatState::default(),
            hub: None,
            watch: None,
            notify_gate: Default::default(),
            start_minimized,
            autostart: crate::autostart::state(),
            tray: None,
            quitting: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            hwnd: Arc::new(AtomicIsize::new(0)),
            refreshed_at: Instant::now(),
            last_resize_check: Instant::now(),
        };
        if let Ok(mut gate) = app.notify_gate.lock() {
            gate.enabled = app.cfg.chat_notifications;
            gate.activity_enabled = app.cfg.activity_notifications;
        }
        // The window handle now, not on the first frame: started hidden at
        // sign-in, the tray's Open needs it before anything is drawn.
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(handle) = cc.window_handle() {
                if let RawWindowHandle::Win32(win32) = handle.as_raw() {
                    app.hwnd.store(win32.hwnd.get(), Ordering::Relaxed);
                    crate::instance::publish(win32.hwnd.get());
                }
            }
        }
        app.start_tray();
        if app.api.has_session() {
            app.load_session();
            app.load_banners();
            // Not waiting for the first frame: started minimised at sign-in, the
            // window may not draw until it is opened.
            app.start_hub();
            app.start_watch();
        }
        app
    }

    pub fn cx(&self) -> Ctx {
        Ctx {
            api: self.api.clone(),
            tx: self.tx.clone(),
            rt: self.rt.clone(),
            egui: self.egui.clone(),
        }
    }

    fn me_id(&self) -> Option<u64> {
        self.session.ready().map(|session| session.user.id)
    }

    // -- loading ------------------------------------------------------------

    pub fn load_session(&mut self) {
        if self.session.is_idle() {
            self.session = Load::Busy;
        }
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::Session(endpoints::session(&api).await.map_err(|error| error.to_string()))
        });
    }

    pub fn load_banners(&mut self) {
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move { Msg::Alert(endpoints::alert(&api).await.ok().flatten()) });

        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move { Msg::Maintenance(endpoints::maintenance(&api).await.ok().flatten()) });
    }

    pub fn load_friends(&mut self) {
        if !self.friends.data.is_idle() {
            return;
        }
        let Some(id) = self.me_id() else { return };
        self.friends.data = Load::Busy;
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::Friends(endpoints::friends_bundle(&api, id).await.map_err(|error| error.to_string()))
        });
    }

    pub fn load_events(&mut self) {
        if !self.home.events.is_idle() {
            return;
        }
        self.home.events = Load::Busy;
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::Events(endpoints::events(&api).await.map_err(|error| error.to_string()))
        });
    }

    pub fn load_sorts(&mut self) {
        if !self.games.sorts.is_idle() {
            return;
        }
        self.games.sorts = Load::Busy;
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::Sorts(endpoints::game_sorts(&api).await.map_err(|error| error.to_string()))
        });
    }

    pub fn load_games(&mut self) {
        if !self.games.games.is_idle() {
            return;
        }
        self.games.games = Load::Busy;
        let cx = self.cx();
        let api = cx.api.clone();
        let sort = self.games.sort.clone();
        let keyword = self.games.keyword.clone();
        cx.spawn(async move {
            Msg::Games(
                endpoints::games_bundle(&api, &sort, &keyword)
                    .await
                    .map_err(|error| error.to_string()),
            )
        });
    }

    pub fn reload_games(&mut self) {
        self.games.games = Load::Idle;
        self.load_games();
    }

    pub fn load_catalog_nav(&mut self) {
        if !self.catalog.nav.is_idle() {
            return;
        }
        self.catalog.nav = Load::Busy;
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::Nav(endpoints::navigation_menu(&api).await.map_err(|error| error.to_string()))
        });
    }

    pub fn load_catalog(&mut self) {
        if !self.catalog.items.is_idle() {
            return;
        }
        let query = endpoints::CatalogQuery {
            keyword: self.catalog.keyword.clone(),
            category: if self.catalog.category.is_empty() {
                "All".to_string()
            } else {
                self.catalog.category.clone()
            },
            ..Default::default()
        };
        self.catalog.cursor = None;
        self.catalog.more_error = None;
        self.catalog.items = Load::Busy;
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::Catalog(
                endpoints::catalog_bundle(&api, &query)
                    .await
                    .map_err(|error| error.to_string()),
            )
        });
    }

    pub fn reload_catalog(&mut self) {
        self.catalog.items = Load::Idle;
        self.load_catalog();
    }

    /// Appends the next page using the cursor the search endpoint handed back.
    pub fn load_more_catalog(&mut self) {
        if self.catalog.loading_more {
            return;
        }
        let Some(cursor) = self.catalog.cursor.clone() else {
            return;
        };
        let query = endpoints::CatalogQuery {
            keyword: self.catalog.keyword.clone(),
            category: if self.catalog.category.is_empty() {
                "All".to_string()
            } else {
                self.catalog.category.clone()
            },
            cursor: Some(cursor),
            ..Default::default()
        };
        self.catalog.loading_more = true;
        self.catalog.more_error = None;
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::CatalogMore(
                endpoints::catalog_bundle(&api, &query)
                    .await
                    .map_err(|error| error.to_string()),
            )
        });
    }

    pub fn load_catalog_detail(&mut self, id: u64) {
        if !self.catalog.detail.is_idle() {
            return;
        }
        self.catalog.detail = Load::Busy;
        self.load_favourite_state(id);
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            let result = endpoints::catalog_details(&api, &[id]).await;
            Msg::CatalogDetail(match result {
                Ok(mut items) => match items.pop() {
                    Some(item) => Ok(item),
                    None => Err("the details endpoint returned nothing".to_string()),
                },
                Err(error) => Err(error.to_string()),
            })
        });
    }

    pub fn load_profile(&mut self) {
        if !self.profile.data.is_idle() {
            return;
        }
        let Some(id) = self.profile.user_id.or_else(|| self.me_id()) else {
            return;
        };
        self.profile.data = Load::Busy;
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::Profile(endpoints::profile_bundle(&api, id).await.map_err(|error| error.to_string()))
        });
    }

    /// Forget any other user's profile so the next load shows your own.
    pub fn show_own_profile(&mut self) {
        if self.profile.user_id.take().is_some() {
            self.profile.data = Load::Idle;
            self.profile.following = Load::Idle;
            self.profile.friendship = Load::Idle;
        }
    }

    pub fn open_profile(&mut self, id: u64) {
        self.profile.user_id = Some(id);
        self.profile.data = Load::Idle;
        self.profile.following = Load::Idle;
        self.profile.friendship = Load::Idle;
        self.view = View::Profile;
        self.load_profile();
    }

    pub fn load_avatar(&mut self) {
        if !self.avatar.data.is_idle() {
            return;
        }
        let Some(id) = self.me_id() else { return };
        self.avatar.data = Load::Busy;
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::Avatar(endpoints::avatar_bundle(&api, id).await.map_err(|error| error.to_string()))
        });
    }

    pub fn load_groups(&mut self) {
        if !self.groups.data.is_idle() {
            return;
        }
        let Some(id) = self.me_id() else { return };
        self.groups.data = Load::Busy;
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::Groups(endpoints::groups_bundle(&api, id).await.map_err(|error| error.to_string()))
        });
    }

    pub fn load_group_detail(&mut self, id: u64) {
        if !self.groups.detail.is_idle() {
            return;
        }
        self.groups.detail = Load::Busy;
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::GroupDetail(
                endpoints::group_detail(&api, id)
                    .await
                    .map_err(|error| error.to_string()),
            )
        });
    }

    pub fn load_inventory(&mut self) {
        if !self.inventory.data.is_idle() {
            return;
        }
        let Some(id) = self.me_id() else { return };
        self.inventory.data = Load::Busy;
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::Inventory(
                endpoints::inventory_bundle(&api, id)
                    .await
                    .map_err(|error| error.to_string()),
            )
        });
    }

    pub fn money_kind(&self) -> String {
        if self.money.kind.is_empty() {
            endpoints::TRANSACTION_TYPES[0].0.to_string()
        } else {
            self.money.kind.clone()
        }
    }

    pub fn load_money(&mut self) {
        if !self.money.data.is_idle() {
            return;
        }
        let Some(id) = self.me_id() else { return };
        let kind = self.money_kind();
        self.money.data = Load::Busy;
        self.money.loading_more = false;
        self.money.more_error = None;
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::Transactions(
                endpoints::transactions(&api, id, &kind, None)
                    .await
                    .map_err(|error| error.to_string()),
            )
        });
    }

    pub fn reload_money(&mut self) {
        self.money.data = Load::Idle;
        self.load_money();
    }

    /// Appends the next page using the cursor the last page handed back.
    pub fn load_more_money(&mut self) {
        if self.money.loading_more {
            return;
        }
        let Some(id) = self.me_id() else { return };
        let Some(cursor) = self.money.data.ready().and_then(|page| page.next.clone()) else {
            return;
        };
        let kind = self.money_kind();
        self.money.loading_more = true;
        self.money.more_error = None;
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::TransactionsMore(
                endpoints::transactions(&api, id, &kind, Some(&cursor))
                    .await
                    .map_err(|error| error.to_string()),
            )
        });
    }

    pub fn messages_tab(&self) -> String {
        if self.messages.tab.is_empty() {
            endpoints::MESSAGE_TABS[0].0.to_string()
        } else {
            self.messages.tab.clone()
        }
    }

    pub fn load_messages(&mut self) {
        if !self.messages.data.is_idle() {
            return;
        }
        let tab = self.messages_tab();
        self.messages.data = Load::Busy;
        self.messages.page = 0;
        self.messages.loading_more = false;
        self.messages.more_error = None;
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::Messages(
                endpoints::messages(&api, &tab, 0)
                    .await
                    .map_err(|error| error.to_string()),
            )
        });
    }

    pub fn reload_messages(&mut self) {
        self.messages.data = Load::Idle;
        self.load_messages();
    }

    pub fn load_more_messages(&mut self) {
        if self.messages.loading_more || !matches!(self.messages.data, Load::Ready(_)) {
            return;
        }
        let tab = self.messages_tab();
        let page = self.messages.page + 1;
        self.messages.loading_more = true;
        self.messages.more_error = None;
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::MessagesMore(
                endpoints::messages(&api, &tab, page)
                    .await
                    .map_err(|error| error.to_string()),
            )
        });
    }

    pub fn trades_tab(&self) -> String {
        if self.trades.tab.is_empty() {
            endpoints::TRADE_TABS[0].0.to_string()
        } else {
            self.trades.tab.clone()
        }
    }

    pub fn load_trades(&mut self) {
        if !self.trades.data.is_idle() {
            return;
        }
        let tab = self.trades_tab();
        self.trades.data = Load::Busy;
        self.trades.loading_more = false;
        self.trades.more_error = None;
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::Trades(
                endpoints::trades(&api, &tab, None)
                    .await
                    .map_err(|error| error.to_string()),
            )
        });
    }

    pub fn reload_trades(&mut self) {
        self.trades.data = Load::Idle;
        self.load_trades();
    }

    pub fn load_more_trades(&mut self) {
        if self.trades.loading_more {
            return;
        }
        let Some(cursor) = self.trades.data.ready().and_then(|page| page.next.clone()) else {
            return;
        };
        let tab = self.trades_tab();
        self.trades.loading_more = true;
        self.trades.more_error = None;
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::TradesMore(
                endpoints::trades(&api, &tab, Some(&cursor))
                    .await
                    .map_err(|error| error.to_string()),
            )
        });
    }

    pub fn open_trade(&mut self, id: u64) {
        self.trades.selected = Some(id);
        self.trades.detail = Load::Idle;
        self.load_trade_detail(id);
    }

    fn load_trade_detail(&mut self, id: u64) {
        self.trades.detail = Load::Busy;
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::TradeDetail(
                id,
                endpoints::trade_detail(&api, id)
                    .await
                    .map_err(|error| error.to_string()),
            )
        });
    }

    pub fn load_friend_requests(&mut self) {
        if !self.friends.requests.is_idle() {
            return;
        }
        self.friends.requests = Load::Busy;
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::FriendRequests(
                endpoints::friend_requests(&api)
                    .await
                    .map_err(|error| error.to_string()),
            )
        });
    }

    fn load_friendship(&mut self, id: u64) {
        let Some(me) = self.me_id() else { return };
        self.profile.friendship = Load::Busy;
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::FriendshipState(
                id,
                endpoints::friendship_status(&api, me, id)
                    .await
                    .map_err(|error| error.to_string()),
            )
        });
    }

    fn load_follow_state(&mut self, id: u64) {
        self.profile.following = Load::Busy;
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::FollowState(
                id,
                endpoints::is_following(&api, id)
                    .await
                    .map_err(|error| error.to_string()),
            )
        });
    }

    fn load_favourite_state(&mut self, asset: u64) {
        let Some(me) = self.me_id() else { return };
        self.catalog.favourite = Load::Busy;
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::FavouriteState(
                asset,
                endpoints::is_favourite(&api, me, asset)
                    .await
                    .map_err(|error| error.to_string()),
            )
        });
    }

    // -- chat -------------------------------------------------------------

    /// Connects to the chat hub once there is a session. Runs until sign-out.
    fn start_hub(&mut self) {
        if self.hub.is_some() || !self.api.has_session() {
            return;
        }
        let tx = self.tx.clone();
        let egui = self.egui.clone();
        let gate = self.notify_gate.clone();
        let hwnd = self.hwnd.clone();
        let click_tx = self.tx.clone();
        let click_egui = self.egui.clone();
        let rt = self.rt.clone();
        let api = self.api.clone();
        let tray = self.tray.clone();
        self.prime_notify_gate();
        self.hub = Some(hub::spawn(&self.rt, self.api.clone(), move |event| {
            // Decided here rather than in the UI loop, which may be asleep while
            // the window is minimised.
            if let HubEvent::Message { conversation_id, user_id, content, .. } = &event {
                let (conversation, sender, content) = (*conversation_id, *user_id, content.clone());
                let (gate, api) = (gate.clone(), api.clone());
                let tray = tray.clone();
                let (hwnd, tx, egui) = (hwnd.clone(), click_tx.clone(), click_egui.clone());
                rt.spawn(async move {
                    // Someone the list has not named yet: look them up first.
                    let known = gate.lock().map(|gate| gate.names.contains_key(&sender)).unwrap_or(true);
                    if !known && sender != 0 {
                        if let Ok(user) = endpoints::user(&api, sender).await {
                            if let Ok(mut gate) = gate.lock() {
                                gate.names.insert(sender, user.name);
                            }
                        }
                    }
                    let (toast, unread) = match gate.lock() {
                        Ok(mut gate) => (
                            crate::notify::decide(&mut gate, conversation, sender, &content, Instant::now()),
                            Some(crate::notify::mark_unread(&mut gate, conversation, sender)),
                        ),
                        Err(_) => (None, None),
                    };
                    if let (Some(tray), Some(unread)) = (&tray, unread) {
                        tray.set_unread(unread);
                    }
                    if let Some((title, body)) = toast {
                        crate::notify::show(title, body, move || {
                            crate::notify::bring_to_front(hwnd.load(Ordering::Relaxed));
                            if tx.send(Msg::OpenChat(conversation)).is_ok() {
                                egui.request_repaint();
                            }
                        });
                    }
                });
            }
            if tx.send(Msg::Hub(event)).is_ok() {
                egui.request_repaint();
            }
        }));
    }

    /// The tray icon. Its actions run on the tray's own thread, so they work
    /// while the window is hidden and not drawing.
    fn start_tray(&mut self) {
        let hwnd = self.hwnd.clone();
        let egui = self.egui.clone();
        let quitting = self.quitting.clone();
        let tray = crate::tray::Tray::spawn(crate::shortcut::write_icon(), move |action| match action {
            crate::tray::Action::Open => {
                crate::notify::bring_to_front(hwnd.load(Ordering::Relaxed));
                egui.request_repaint();
            }
            crate::tray::Action::Quit => {
                quitting.store(true, Ordering::Relaxed);
                crate::tray::remove_icon();
                egui.send_viewport_cmd(egui::ViewportCommand::Close);
                egui.request_repaint();
                // A hidden window may never draw the frame that closes it.
                std::thread::spawn(|| {
                    std::thread::sleep(Duration::from_millis(1500));
                    std::process::exit(0);
                });
            }
        });
        self.tray = tray.map(Arc::new);
    }

    /// Fills in who you are and everyone's names for notifications, without
    /// waiting for the UI to load them.
    fn prime_notify_gate(&mut self) {
        let (gate, api) = (self.notify_gate.clone(), self.api.clone());
        self.rt.spawn(async move {
            let Ok(user) = endpoints::authenticated_user(&api).await else { return };
            if let Ok(mut gate) = gate.lock() {
                gate.me = user.id;
            }
            if let Ok(bundle) = endpoints::chat_bundle(&api, user.id).await {
                if let Ok(mut gate) = gate.lock() {
                    crate::notify::learn(&mut gate, &bundle, user.id);
                }
            }
        });
    }

    /// Settings: turn "Open at sign-in" on or off.
    pub fn set_autostart(&mut self, enabled: bool) {
        if let Err(error) = crate::autostart::set(enabled) {
            self.notice = Some((false, error));
        }
        self.autostart = crate::autostart::state();
    }

    /// Notifications for trades, friend requests and messages (no live push for
    /// those, so a once-a-minute watcher).
    fn start_watch(&mut self) {
        if self.watch.is_some() || !self.api.has_session() {
            return;
        }
        let gate = self.notify_gate.clone();
        let hwnd = self.hwnd.clone();
        let (tx, egui) = (self.tx.clone(), self.egui.clone());
        let (changed_tx, changed_egui) = (self.tx.clone(), self.egui.clone());
        let report = move |kind: crate::watch::Kind, items: Vec<crate::watch::Item>| {
            let view = match kind {
                crate::watch::Kind::Trade => View::Trades,
                crate::watch::Kind::FriendRequest => View::Friends,
                crate::watch::Kind::Message => View::Messages,
            };
            let quiet = gate
                .lock()
                .map(|gate| !gate.activity_enabled || (gate.focused && gate.screen == view.label()))
                .unwrap_or(true);
            if quiet {
                return;
            }
            let Some((title, body)) = crate::watch::toast_text(kind, &items) else { return };
            let (hwnd, tx, egui) = (hwnd.clone(), tx.clone(), egui.clone());
            crate::notify::show(title, body, move || {
                crate::notify::bring_to_front(hwnd.load(Ordering::Relaxed));
                if tx.send(Msg::OpenView(view)).is_ok() {
                    egui.request_repaint();
                }
            });
        };
        let changed = move || {
            if changed_tx.send(Msg::ActivityChanged).is_ok() {
                changed_egui.request_repaint();
            }
        };
        self.watch = Some(crate::watch::spawn(&self.rt, self.api.clone(), report, changed));
    }

    /// The Play button: fetch a join ticket and start the Hexium launcher.
    pub fn play(&mut self, place_id: u64, job_id: Option<String>) {
        if matches!(self.games.launch, Some((_, Load::Busy))) {
            return;
        }
        self.games.launch = Some((place_id, Load::Busy));
        self.games.launch_job = job_id.clone();
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::GameLaunched(place_id, crate::play::launch(&api, place_id, job_id.as_deref()).await)
        });
    }

    /// Servers of a place; `start` 0 replaces the list, more appends.
    /// Join friend: find which server they are on, then join it.
    pub fn join_friend(&mut self, user_id: u64, name: String, place_id: u64) {
        if matches!(self.friends.join, Some((_, Load::Busy))) {
            return;
        }
        self.friends.join = Some((user_id, Load::Busy));
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            let result = match endpoints::find_server_of(&api, place_id, user_id).await {
                Ok(Some(job)) => crate::play::launch(&api, place_id, Some(&job)).await,
                Ok(None) => Err(format!(
                    "Could not find {name}'s server. They may have just left, or be on a private server."
                )),
                Err(error) => Err(error.to_string()),
            };
            Msg::FriendJoined(user_id, result)
        });
    }

    pub fn load_servers(&mut self, place_id: u64, start: u64) {
        if start == 0 {
            self.games.servers = Load::Busy;
        } else {
            self.games.servers_more = true;
        }
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::GameServers(
                place_id,
                start,
                endpoints::game_servers(&api, place_id, start).await.map_err(|error| error.to_string()),
            )
        });
    }

    fn stop_hub(&mut self) {
        if let Some(watch) = self.watch.take() {
            watch.stop();
        }
        if let Some(handle) = self.hub.take() {
            handle.stop();
        }
        self.chat.hub = None;
    }

    pub fn load_chat(&mut self) {
        if !self.chat.list.is_idle() {
            return;
        }
        let Some(me) = self.me_id() else { return };
        self.chat.list = Load::Busy;
        self.refresh_chat_list(me);
    }

    /// Refetches the list without blanking what is on screen.
    fn refresh_chat_list(&mut self, me: u64) {
        self.chat.listed_at = Some(Instant::now());
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::ChatList(endpoints::chat_bundle(&api, me).await.map_err(|error| error.to_string()))
        });
    }

    pub fn open_chat(&mut self, target: ChatTarget) {
        if self.chat.target.as_ref() == Some(&target) {
            return;
        }
        self.stop_typing();
        self.chat.draft.clear();
        self.chat.messages = match &target {
            ChatTarget::Conversation(_) => Load::Busy,
            ChatTarget::NewWith { .. } => Load::Ready(Vec::new()),
        };
        self.chat.target = Some(target);
        if let Some(id) = self.chat.open_conversation() {
            self.fetch_chat_messages(id);
        }
    }

    /// Chat with a user from elsewhere (a profile): their conversation if there
    /// is one, otherwise a new one that starts with the first message.
    pub fn chat_with(&mut self, user_id: u64, name: &str) {
        let me = self.me_id().unwrap_or(0);
        let existing = self.chat.list.ready().and_then(|bundle| {
            bundle
                .conversations
                .iter()
                .find(|conversation| {
                    conversation.kind != "MultiUserConversation"
                        && conversation.other(me).map(|other| other.id) == Some(user_id)
                })
                .map(|conversation| conversation.id)
        });
        self.view = View::Chat;
        self.on_view_enter();
        match existing {
            Some(id) => self.open_chat(ChatTarget::Conversation(id)),
            None => self.open_chat(ChatTarget::NewWith { user_id, name: name.to_string() }),
        }
    }

    fn fetch_chat_messages(&mut self, conversation: u64) {
        self.chat.polled_at = Some(Instant::now());
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::ChatMessages(
                conversation,
                endpoints::chat_messages(&api, conversation).await.map_err(|error| error.to_string()),
            )
        });
    }

    /// What the site does on its own while a conversation is open: mark up to
    /// the newest message read. Sent once per newest message.
    fn mark_chat_read(&mut self, conversation: u64) {
        let Some(last) = self.chat.messages.ready().and_then(|messages| messages.last()).map(|message| message.id.clone()) else {
            return;
        };
        if last.is_empty() || self.chat.marked.get(&conversation) == Some(&last) {
            return;
        }
        let unread = self
            .chat
            .list
            .ready()
            .and_then(|bundle| bundle.conversations.iter().find(|item| item.id == conversation))
            .is_some_and(|item| item.unread);
        let me = self.me_id().unwrap_or(0);
        let theirs = self
            .chat
            .messages
            .ready()
            .and_then(|messages| messages.last())
            .is_some_and(|message| message.sender_id != me && !message.read);
        self.chat.marked.insert(conversation, last.clone());
        if !unread && !theirs {
            return;
        }
        if let Load::Ready(bundle) = &mut self.chat.list {
            if let Some(item) = bundle.conversations.iter_mut().find(|item| item.id == conversation) {
                item.unread = false;
            }
        }
        let api = self.api.clone();
        self.rt.spawn(async move {
            let _ = endpoints::chat_mark_read(&api, conversation, &last).await;
        });
    }

    /// Adds a message to its conversation in the list (and the open thread),
    /// newest conversation first.
    fn place_chat_message(&mut self, conversation: u64, message: models::ChatMessage) -> bool {
        let me = self.me_id().unwrap_or(0);
        let open = self.chat.open_conversation() == Some(conversation);
        if open {
            if let Load::Ready(messages) = &mut self.chat.messages {
                let known = !message.id.is_empty() && messages.iter().any(|old| old.id == message.id);
                if !known {
                    messages.push(message.clone());
                    self.chat.arrivals += 1;
                }
            }
        }
        let Load::Ready(bundle) = &mut self.chat.list else { return false };
        let Some(index) = bundle.conversations.iter().position(|item| item.id == conversation) else {
            return false;
        };
        let mut item = bundle.conversations.remove(index);
        item.unread = !open && message.sender_id != me && message.sender_id != 0;
        item.latest = Some(message);
        bundle.conversations.insert(0, item);
        true
    }

    fn on_hub(&mut self, event: HubEvent) {
        match event {
            HubEvent::Status(status) => {
                // Coming back online may have missed messages.
                let reconnected = status == HubStatus::Live && self.chat.hub.is_some() && !self.chat.hub_live();
                self.chat.hub = Some(status);
                if reconnected {
                    if let Some(me) = self.me_id() {
                        if self.chat.list.ready().is_some() {
                            self.refresh_chat_list(me);
                        }
                    }
                    if let Some(id) = self.chat.open_conversation() {
                        self.fetch_chat_messages(id);
                    }
                }
            }
            HubEvent::Message { conversation_id, id, content, sent, user_id } => {
                self.chat.typing.remove(&conversation_id);
                let message = models::ChatMessage { id, content, sent, sender_id: user_id, read: false };
                let placed = self.place_chat_message(conversation_id, message);
                if !placed {
                    // A conversation the list does not have yet.
                    if let Some(me) = self.me_id() {
                        self.refresh_chat_list(me);
                    }
                }
                if self.view == View::Chat && self.chat.open_conversation() == Some(conversation_id) {
                    self.mark_chat_read(conversation_id);
                }
            }
            HubEvent::Typing { conversation_id, user_id, ends_at_ms } => {
                if Some(user_id) != self.me_id() {
                    self.chat.typing.insert(conversation_id, (user_id, ends_at_ms));
                }
            }
            HubEvent::ConversationAdded => {
                if let Some(me) = self.me_id() {
                    self.refresh_chat_list(me);
                }
            }
        }
    }

    /// Keeps what the hub thread needs to decide on a toast current.
    fn sync_notify_gate(&mut self, ctx: &egui::Context) {
        let focused = ctx.input(|input| input.viewport().focused.unwrap_or(true));
        let minimized = ctx.input(|input| input.viewport().minimized.unwrap_or(false));
        let watching = if self.view == View::Chat { self.chat.open_conversation() } else { None };
        let me = self.me_id().unwrap_or(0);
        if let Ok(mut gate) = self.notify_gate.lock() {
            gate.enabled = self.cfg.chat_notifications;
            if me != 0 {
                gate.me = me;
            }
            gate.focused = focused && !minimized;
            gate.watching = watching;
            gate.activity_enabled = self.cfg.activity_notifications;
            gate.screen = if focused && !minimized { self.view.label() } else { "" };
            // While the UI runs, the chat list is the truth.
            if let Some(bundle) = self.chat.list.ready() {
                gate.unread = bundle
                    .conversations
                    .iter()
                    .filter(|conversation| conversation.unread)
                    .map(|conversation| conversation.id)
                    .collect();
            }
            if let Some(tray) = &self.tray {
                tray.set_unread(gate.unread.len() as u64);
            }
        }
    }

    /// Closing the window: hide it to the tray, unless that is switched off,
    /// there is no tray, or Quit was chosen.
    fn handle_close(&mut self, ctx: &egui::Context) {
        if !ctx.input(|input| input.viewport().close_requested()) {
            return;
        }
        let quitting = self.quitting.load(Ordering::Relaxed);
        if self.cfg.close_to_tray && self.tray.is_some() && !quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            crate::tray::hide_window(self.hwnd.load(Ordering::Relaxed));
            if let Ok(mut gate) = self.notify_gate.lock() {
                gate.focused = false;
            }
        } else {
            crate::tray::remove_icon();
        }
    }

    /// A key was pressed in the composer. Like the site, only conversations that
    /// already exist get typing updates.
    pub fn note_typing(&mut self) {
        if !self.cfg.share_typing {
            return;
        }
        let Some(id) = self.chat.open_conversation() else { return };
        if self.chat.typing_in != Some(id) {
            self.chat.typing_sent = None;
        }
        self.chat.typing_in = Some(id);
        self.chat.last_key = Some(Instant::now());
        self.broadcast_typing();
    }

    /// The message went out or the conversation changed: stop broadcasting.
    pub fn stop_typing(&mut self) {
        self.chat.typing_in = None;
        self.chat.last_key = None;
        self.chat.typing_sent = None;
    }

    /// Sends `isTyping` at most once a second until 3 s after the last key,
    /// the site's own timings.
    fn broadcast_typing(&mut self) {
        let (Some(id), Some(last_key)) = (self.chat.typing_in, self.chat.last_key) else { return };
        if last_key.elapsed() >= Duration::from_secs(3)
            || self.view != View::Chat
            || self.chat.open_conversation() != Some(id)
            || !self.cfg.share_typing
        {
            self.stop_typing();
            return;
        }
        if self.chat.typing_sent.is_some_and(|at| at.elapsed() < Duration::from_secs(1)) {
            return;
        }
        self.chat.typing_sent = Some(Instant::now());
        let api = self.api.clone();
        self.rt.spawn(async move {
            // Best effort, as on the site: a missed update only hides the dots.
            let _ = endpoints::chat_typing(&api, id).await;
        });
    }

    /// Polling the site itself uses (6 s for the open conversation, 60 s for the
    /// list). While the hub is live the open conversation is only checked every
    /// 30 s, as a safety net.
    fn poll_chat(&mut self) {
        let Some(me) = self.me_id() else { return };
        self.broadcast_typing();
        let live = self.chat.hub_live();
        if self.view == View::Chat {
            if let Some(id) = self.chat.open_conversation() {
                let every = Duration::from_secs(if live { 30 } else { 6 });
                let due = self.chat.polled_at.map_or(true, |at| at.elapsed() >= every);
                if due && self.chat.messages.ready().is_some() {
                    self.fetch_chat_messages(id);
                }
            }
        }
        let due = self.chat.listed_at.map_or(false, |at| at.elapsed() >= Duration::from_secs(60));
        if due && self.chat.list.ready().is_some() {
            self.refresh_chat_list(me);
        }
    }

    /// Opens the confirmation dialog. Nothing is sent yet.
    pub fn ask(&mut self, action: WriteAction) {
        if self.pending_write.as_ref().is_some_and(|pending| pending.sending) {
            return;
        }
        self.pending_write = Some(PendingWrite {
            action,
            sending: false,
            error: None,
        });
    }

    pub fn cancel_write(&mut self) {
        if self.pending_write.as_ref().is_some_and(|pending| !pending.sending) {
            self.pending_write = None;
        }
    }

    /// The user pressed the confirm button: send it, once.
    pub fn confirm_write(&mut self) {
        let Some(pending) = self.pending_write.as_mut() else { return };
        if pending.sending {
            return;
        }
        let Some(me) = self.session.ready().map(|session| session.user.id) else {
            pending.error = Some("Your session is not loaded yet.".to_string());
            return;
        };
        pending.sending = true;
        pending.error = None;
        let action = pending.action.clone();
        if let WriteAction::SendChat { conversation_id, user_id, text, .. } = &action {
            let (conversation_id, user_id, text) = (*conversation_id, *user_id, text.clone());
            let tx = self.tx.clone();
            let cx = self.cx();
            let api = cx.api.clone();
            cx.spawn(async move {
                let sent = async {
                    let conversation = match conversation_id {
                        Some(id) => models::ChatConversation { id, ..Default::default() },
                        None => endpoints::chat_start(&api, user_id).await?,
                    };
                    let message = endpoints::chat_send(&api, conversation.id, &text).await?;
                    Ok::<_, crate::api::ApiError>((conversation, message))
                }
                .await;
                let result = match sent {
                    Ok((conversation, message)) => {
                        let _ = tx.send(Msg::ChatSent(conversation, message));
                        Ok(())
                    }
                    Err(crate::api::ApiError::Network(detail)) => Err(format!(
                        "{detail}. The message may or may not have gone through; wait for the chat to refresh before sending it again."
                    )),
                    Err(other) => Err(other.to_string()),
                };
                Msg::WriteDone(action, result)
            });
            return;
        }
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            let result = match &action {
                WriteAction::Follow { user_id, follow, .. } => {
                    endpoints::set_following(&api, *user_id, *follow).await
                }
                WriteAction::Favourite { asset_id, favourite, .. } => {
                    endpoints::set_favourite(&api, me, *asset_id, *favourite).await
                }
                WriteAction::SetStatus { status } => endpoints::set_status(&api, me, status).await,
                WriteAction::Archive { message_id, archive, .. } => {
                    endpoints::set_messages_archived(&api, &[*message_id], *archive).await
                }
                WriteAction::MarkAllRead { ids } => endpoints::mark_all_read(&api, ids).await,
                WriteAction::VoteGame { universe_id, up, .. } => {
                    endpoints::vote_game(&api, *universe_id, *up).await
                }
                WriteAction::SendTrade { partner_id, counter_of, give_ids, get_ids, .. } => {
                    endpoints::send_trade(&api, me, *partner_id, give_ids, get_ids, *counter_of).await
                }
                WriteAction::SetDescription { description } => {
                    endpoints::set_description(&api, description).await
                }
                WriteAction::MarkRead { message_id, read, .. } => {
                    endpoints::set_messages_read(&api, &[*message_id], *read).await
                }
                WriteAction::RequestFriend { user_id, .. } => {
                    endpoints::friendship_write(&api, *user_id, "request-friendship").await
                }
                WriteAction::AcceptFriend { user_id, .. } => {
                    endpoints::friendship_write(&api, *user_id, "accept-friend-request").await
                }
                WriteAction::DeclineFriend { user_id, .. } => {
                    endpoints::friendship_write(&api, *user_id, "decline-friend-request").await
                }
                WriteAction::Unfriend { user_id, .. } => {
                    endpoints::friendship_write(&api, *user_id, "unfriend").await
                }
                WriteAction::SetResalePrice { asset_id, user_asset_id, new, .. } => {
                    endpoints::set_resale_price(&api, *asset_id, *user_asset_id, new.unwrap_or(0)).await
                }
                WriteAction::AnswerTrade { trade_id, accept, .. } => {
                    endpoints::answer_trade(&api, *trade_id, *accept).await
                }
                WriteAction::SendMessage { recipient_id, subject, body, reply_to, include_previous, .. } => {
                    endpoints::send_message(&api, *recipient_id, subject, body, *reply_to, *include_previous).await
                }
                // Handled above.
                WriteAction::SendChat { .. } => Ok(()),
            };
            let result = result.map_err(|error| match error {
                // A write that lost its connection may still have landed.
                crate::api::ApiError::Network(detail) => format!(
                    "{detail}. The change may or may not have gone through; refresh to check before trying again."
                ),
                other => other.to_string(),
            });
            Msg::WriteDone(action, result)
        });
    }

    fn finish_write(&mut self, action: WriteAction, result: Res<()>) {
        if let Err(error) = result {
            // Keep the dialog open so the error is read where the action was asked.
            match self.pending_write.as_mut() {
                Some(pending) => {
                    pending.sending = false;
                    pending.error = Some(error);
                }
                None => self.notice = Some((false, format!("{} failed: {error}", action.title()))),
            }
            return;
        }

        self.pending_write = None;
        let message = match &action {
            WriteAction::Follow { user_id, name, follow } => {
                if self.profile.data.ready().map(|bundle| bundle.user.id) == Some(*user_id) {
                    self.profile.following = Load::Ready(*follow);
                    if let Load::Ready(bundle) = &mut self.profile.data {
                        if *follow {
                            bundle.followers += 1;
                        } else {
                            bundle.followers = bundle.followers.saturating_sub(1);
                        }
                    }
                }
                if *follow {
                    format!("You now follow {name}.")
                } else {
                    format!("You no longer follow {name}.")
                }
            }
            WriteAction::Favourite { asset_id, name, favourite } => {
                if self.catalog.selected.as_ref().map(|item| item.id) == Some(*asset_id) {
                    self.catalog.favourite = Load::Ready(*favourite);
                }
                if *favourite {
                    format!("Added \"{name}\" to your favourites.")
                } else {
                    format!("Removed \"{name}\" from your favourites.")
                }
            }
            WriteAction::SetStatus { status } => {
                let me = self.me_id();
                if let Load::Ready(bundle) = &mut self.profile.data {
                    if Some(bundle.user.id) == me {
                        bundle.status = status.clone();
                    }
                }
                if status.trim().is_empty() {
                    "Status cleared.".to_string()
                } else {
                    "Status updated.".to_string()
                }
            }
            WriteAction::Archive { message_id, subject, archive } => {
                // It now lives in the other tab, so it leaves this one.
                if let Load::Ready(page) = &mut self.messages.data {
                    let before = page.items.len();
                    page.items.retain(|message| message.id != *message_id);
                    if page.items.len() < before {
                        page.total = page.total.saturating_sub(1);
                    }
                }
                self.load_session();
                if *archive {
                    format!("Moved \"{subject}\" to Archive.")
                } else {
                    format!("Moved \"{subject}\" back to your inbox.")
                }
            }
            WriteAction::SendTrade { partner, counter_of, .. } => {
                self.trade_builder = None;
                if let Some(id) = counter_of {
                    // The countered offer is no longer open for you to answer.
                    if let Load::Ready(page) = &mut self.trades.data {
                        page.items.retain(|trade| trade.id != *id);
                    }
                    if self.trades.selected == Some(*id) {
                        self.trades.selected = None;
                        self.trades.detail = Load::Idle;
                    }
                }
                if self.trades_tab() == "outbound" {
                    self.trades.data = Load::Idle;
                }
                self.load_session();
                if counter_of.is_some() {
                    format!("Counter offer sent to {partner}.")
                } else {
                    format!("Trade sent to {partner}.")
                }
            }
            WriteAction::VoteGame { universe_id, name, up, previous } => {
                let up = *up;
                // Move the counts the same way the site will.
                let adjust = |votes_up: &mut u64, votes_down: &mut u64| {
                    if *previous == Some(!up) {
                        if up {
                            *votes_down = votes_down.saturating_sub(1);
                        } else {
                            *votes_up = votes_up.saturating_sub(1);
                        }
                    }
                    if *previous != Some(up) {
                        if up {
                            *votes_up += 1;
                        } else {
                            *votes_down += 1;
                        }
                    }
                };
                if let Load::Ready(state) = &mut self.games.vote {
                    adjust(&mut state.up, &mut state.down);
                    state.user_vote = Some(up);
                }
                if let Load::Ready(bundle) = &mut self.games.games {
                    if let Some(game) = bundle.games.iter_mut().find(|game| game.id == *universe_id) {
                        adjust(&mut game.votes_up, &mut game.votes_down);
                    }
                }
                let which = if up { "thumbs up" } else { "thumbs down" };
                format!("Gave \"{name}\" a {which}.")
            }
            WriteAction::SetDescription { description } => {
                let me = self.me_id();
                if let Load::Ready(bundle) = &mut self.profile.data {
                    if Some(bundle.user.id) == me {
                        bundle.user.description = description.clone();
                    }
                }
                if description.trim().is_empty() {
                    "Description cleared.".to_string()
                } else {
                    "Description updated.".to_string()
                }
            }
            WriteAction::MarkAllRead { ids } => {
                if self.messages_tab() == "inbox" {
                    if let Load::Ready(page) = &mut self.messages.data {
                        for message in &mut page.items {
                            message.is_read = true;
                        }
                    }
                }
                self.load_session();
                let noun = if ids.len() == 1 { "message" } else { "messages" };
                format!("Marked {} {noun} as read.", ids.len())
            }
            WriteAction::MarkRead { message_id, read, .. } => {
                if let Load::Ready(page) = &mut self.messages.data {
                    for message in &mut page.items {
                        if message.id == *message_id {
                            message.is_read = *read;
                        }
                    }
                }
                // The unread counter on Home comes from the session.
                self.load_session();
                if *read {
                    "Marked as read.".to_string()
                } else {
                    "Marked as unread.".to_string()
                }
            }
            WriteAction::RequestFriend { user_id, name } => {
                self.set_friendship(*user_id, "RequestSent");
                format!("Friend request sent to {name}.")
            }
            WriteAction::AcceptFriend { user_id, name } => {
                self.drop_request(*user_id);
                self.set_friendship(*user_id, "Friends");
                self.friends.data = Load::Idle;
                self.load_session();
                format!("You are now friends with {name}.")
            }
            WriteAction::DeclineFriend { user_id, name } => {
                self.drop_request(*user_id);
                self.set_friendship(*user_id, "NotFriends");
                self.load_session();
                format!("Declined {name}'s friend request.")
            }
            WriteAction::Unfriend { user_id, name } => {
                self.set_friendship(*user_id, "NotFriends");
                self.friends.data = Load::Idle;
                format!("{name} is no longer your friend.")
            }
            WriteAction::SetResalePrice { user_asset_id, name, old, new, .. } => {
                if let Some(resale) = self.inventory.resale.as_mut() {
                    if resale.user_asset_id == *user_asset_id {
                        if let Load::Ready(info) = &mut resale.info {
                            info.my_price = *new;
                        }
                        resale.price_input = new.map(|price| price.to_string()).unwrap_or_default();
                    }
                }
                match (old, new) {
                    (_, None) => format!("\"{name}\" is off sale."),
                    (None, Some(price)) => {
                        format!("\"{name}\" is listed for {} R$.", crate::ui::money(*price as i64))
                    }
                    (Some(_), Some(price)) => {
                        format!("\"{name}\" now costs {} R$.", crate::ui::money(*price as i64))
                    }
                }
            }
            WriteAction::AnswerTrade { trade_id, partner, accept, .. } => {
                // It is no longer open, so it leaves the list and the window closes.
                if let Load::Ready(page) = &mut self.trades.data {
                    page.items.retain(|trade| trade.id != *trade_id);
                }
                self.trades.selected = None;
                self.trades.detail = Load::Idle;
                // The inbound count on Home and in the menu badge.
                self.load_session();
                if *accept {
                    // Items changed hands.
                    self.inventory.data = Load::Idle;
                    format!("Trade with {partner} accepted. The items have moved.")
                } else {
                    format!("Declined {partner}'s trade.")
                }
            }
            WriteAction::SendMessage { recipient, .. } => {
                self.compose = None;
                // A fresh Sent tab should show it.
                if self.messages_tab() == "sent" {
                    self.messages.data = Load::Idle;
                }
                format!("Message sent to {recipient}.")
            }
            WriteAction::SendChat { .. } => {
                // The thread already shows it (Msg::ChatSent); no banner needed.
                return;
            }
        };
        self.notice = Some((true, message));
    }

    fn set_friendship(&mut self, user_id: u64, status: &str) {
        if self.profile.data.ready().map(|bundle| bundle.user.id) == Some(user_id) {
            self.profile.friendship = Load::Ready(status.to_string());
        }
    }

    fn drop_request(&mut self, user_id: u64) {
        if let Load::Ready(bundle) = &mut self.friends.requests {
            bundle.items.retain(|request| request.user_id != user_id);
        }
    }

    /// Opens a blank message to someone. Replaces any unsent draft only if the
    /// draft is empty, so a half-written message is not lost by a stray click.
    pub fn compose_to(&mut self, recipient_id: u64, recipient: &str) {
        if self.compose_has_text() {
            self.notice = Some((false, "Finish or close the message you are writing first.".to_string()));
            return;
        }
        self.compose = Some(Compose {
            recipient_id,
            recipient: recipient.to_string(),
            ..Default::default()
        });
    }

    pub fn compose_reply(&mut self, message: &models::Message) {
        if self.compose_has_text() {
            self.notice = Some((false, "Finish or close the message you are writing first.".to_string()));
            return;
        }
        let subject = message.subject.trim();
        let subject = if subject.to_ascii_lowercase().starts_with("re:") {
            subject.to_string()
        } else {
            format!("RE: {subject}")
        };
        self.compose = Some(Compose {
            recipient_id: message.sender_id,
            recipient: message.sender_name.clone(),
            subject,
            body: String::new(),
            reply_to: Some(message.id),
            include_previous: true,
        });
    }

    fn compose_has_text(&self) -> bool {
        self.compose
            .as_ref()
            .is_some_and(|draft| !draft.body.trim().is_empty())
    }

    /// Opens the Sell window for one collectible copy and loads its market.
    pub fn open_resale(&mut self, item: &models::Collectible, icon: Option<String>) {
        let Some(me) = self.me_id() else { return };
        self.inventory.resale = Some(ResaleState {
            asset_id: item.id,
            user_asset_id: item.user_asset_id,
            name: item.name.clone(),
            serial: item.serial,
            rap: item.rap,
            icon,
            info: Load::Busy,
            price_input: String::new(),
        });
        let (asset, copy) = (item.id, item.user_asset_id);
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::Resale(
                copy,
                endpoints::resale_info(&api, me, asset, copy)
                    .await
                    .map_err(|error| error.to_string()),
            )
        });
    }

    /// Gathers every unread inbox id, then opens the confirmation with the count.
    pub fn prepare_mark_all_read(&mut self) {
        if self.messages.preparing_all {
            return;
        }
        self.messages.preparing_all = true;
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::UnreadIds(
                endpoints::unread_inbox_ids(&api)
                    .await
                    .map_err(|error| error.to_string()),
            )
        });
    }

    fn selected_game_id(&self) -> Option<u64> {
        let index = self.games.selected?;
        self.games
            .games
            .ready()
            .and_then(|bundle| bundle.games.get(index))
            .map(|game| game.id)
    }

    /// Opens a game's details window and loads its votes.
    pub fn open_game(&mut self, index: usize) {
        self.games.selected = Some(index);
        self.games.vote = Load::Busy;
        self.games.servers = Load::Idle;
        let place = self
            .games
            .games
            .ready()
            .and_then(|bundle| bundle.games.get(index))
            .and_then(|game| game.place_id);
        if let Some(place) = place {
            self.load_servers(place, 0);
        }
        // To mark friends in the server list.
        self.load_friends();
        let Some(universe) = self.selected_game_id() else {
            self.games.vote = Load::Idle;
            return;
        };
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move {
            Msg::GameVote(
                universe,
                endpoints::game_vote_state(&api, universe)
                    .await
                    .map_err(|error| error.to_string()),
            )
        });
    }

    /// Opens the trade builder with `partner`, optionally prefilled (a counter
    /// starts from the offer being countered), and loads both inventories.
    pub fn open_trade_builder(
        &mut self,
        partner_id: u64,
        partner: &str,
        counter_of: Option<u64>,
        give: Vec<u64>,
        get: Vec<u64>,
    ) {
        let Some(me) = self.me_id() else { return };
        if partner_id == me {
            return;
        }
        self.trade_builder = Some(TradeBuilder {
            partner_id,
            partner: partner.to_string(),
            counter_of,
            mine: Load::Busy,
            theirs: Load::Busy,
            give,
            get,
            filter_mine: String::new(),
            filter_theirs: String::new(),
        });
        for owner in [me, partner_id] {
            let cx = self.cx();
            let api = cx.api.clone();
            cx.spawn(async move {
                Msg::TradeInventory {
                    owner,
                    result: endpoints::trade_inventory(&api, owner)
                        .await
                        .map_err(|error| error.to_string()),
                }
            });
        }
    }

    pub fn probe(&mut self) {
        let cx = self.cx();
        let api = cx.api.clone();
        cx.spawn(async move { Msg::Probe(api.probe().await.unwrap_or(false)) });
    }

    // -- lifecycle ----------------------------------------------------------

    pub fn reload_current_view(&mut self) {
        match self.view {
            View::Home => {
                self.friends.data = Load::Idle;
                self.home.events = Load::Idle;
            }
            View::Games => {
                self.games.games = Load::Idle;
                self.games.sorts = Load::Idle;
            }
            View::Catalog => {
                self.catalog.items = Load::Idle;
                self.catalog.nav = Load::Idle;
                self.catalog.cursor = None;
            }
            View::Profile => self.profile.data = Load::Idle,
            View::Friends => {
                self.friends.data = Load::Idle;
                self.friends.requests = Load::Idle;
            }
            View::Avatar => self.avatar.data = Load::Idle,
            View::Groups => {
                self.groups.data = Load::Idle;
                self.groups.detail = Load::Idle;
            }
            View::Inventory => self.inventory.data = Load::Idle,
            View::Money => self.money.data = Load::Idle,
            View::Messages => self.messages.data = Load::Idle,
            View::Chat => {
                self.chat.list = Load::Idle;
                if let Some(id) = self.chat.open_conversation() {
                    self.fetch_chat_messages(id);
                }
            }
            View::Trades => {
                self.trades.data = Load::Idle;
                self.trades.detail = Load::Idle;
                if let Some(id) = self.trades.selected {
                    self.load_trade_detail(id);
                }
            }
            View::Settings => {}
        }
        self.on_view_enter();
        self.load_banners();
    }

    pub fn on_view_enter(&mut self) {
        match self.view {
            View::Home => {
                self.load_friends();
                self.load_events();
            }
            View::Games => {
                self.load_sorts();
                self.load_games();
            }
            View::Catalog => {
                self.load_catalog_nav();
                self.load_catalog();
            }
            View::Profile => self.load_profile(),
            View::Friends => self.load_friends(),
            View::Avatar => self.load_avatar(),
            View::Groups => self.load_groups(),
            View::Inventory => self.load_inventory(),
            View::Money => self.load_money(),
            View::Messages => self.load_messages(),
            View::Chat => self.load_chat(),
            View::Trades => self.load_trades(),
            View::Settings => {}
        }
    }

    pub fn sign_in(&mut self) {
        let cookie = Config::normalize_cookie(&self.login_input);
        if cookie.is_empty() {
            self.auth_error = Some("Paste the .ROBLOSECURITY cookie value first.".to_string());
            return;
        }
        self.api.set_session(Some(&cookie));
        self.cfg.session_cookie = Some(cookie);
        self.cfg.save();
        self.login_input.clear();
        self.auth_error = None;
        self.stale_error = None;
        self.session = Load::Idle;
        self.load_session();
        self.load_banners();
    }

    pub fn sign_out(&mut self) {
        self.stop_hub();
        if let Ok(mut gate) = self.notify_gate.lock() {
            *gate = crate::notify::Gate::default();
        }
        self.api.set_session(None);
        self.cfg.session_cookie = None;
        self.cfg.save();
        self.session = Load::Idle;
        self.auth_error = None;
        self.stale_error = None;
        self.probe_result = None;
        self.pending_write = None;
        self.compose = None;
        self.trade_builder = None;
        self.notice = None;
        self.alert = None;
        self.maintenance = None;
        self.thumbs = Thumbs::default();
        self.friends = FriendsState::default();
        self.home = HomeState::default();
        self.games = GamesState::default();
        self.catalog = CatalogState::default();
        self.profile = ProfileState::default();
        self.avatar = AvatarState::default();
        self.groups = GroupsState::default();
        self.inventory = InventoryState::default();
        self.money = MoneyState::default();
        self.messages = MessagesState::default();
        self.trades = TradesState::default();
        self.chat = ChatState::default();
        self.view = View::Home;
    }

    fn apply(&mut self, message: Msg) {
        match message {
            Msg::Session(result) => match result {
                Ok(session) => {
                    self.session = Load::Ready(session);
                    self.start_hub();
                    self.start_watch();
                    self.auth_error = None;
                    self.stale_error = None;
                    self.refreshed_at = Instant::now();
                }
                Err(error) => {
                    if self.session.ready().is_some() {
                        self.stale_error = Some(error);
                    } else {
                        self.session = Load::Err(error.clone());
                        self.auth_error = Some(error);
                    }
                }
            },
            Msg::Thumb(url, bytes) => self.thumbs.apply(url, bytes),
            Msg::Alert(message) => self.alert = message,
            Msg::Maintenance(message) => self.maintenance = message,
            Msg::Events(result) => self.home.events = into_load(result),
            Msg::Friends(result) => self.friends.data = into_load(result),
            Msg::Sorts(result) => {
                if let Ok(sorts) = &result {
                    if self.games.sort.is_empty() {
                        self.games.sort = sorts.first().map(|(token, _)| token.clone()).unwrap_or_default();
                    }
                }
                self.games.sorts = into_load(result);
            }
            Msg::Games(result) => self.games.games = into_load(result),
            Msg::Nav(result) => {
                if let Ok(entries) = &result {
                    if self.catalog.category.is_empty() && !entries.is_empty() {
                        self.catalog.category = entries[0].0.clone();
                    }
                }
                self.catalog.nav = into_load(result);
            }
            Msg::Catalog(result) => {
                if let Ok(bundle) = &result {
                    self.catalog.cursor = bundle.next.clone();
                    self.catalog.more_error = None;
                }
                self.catalog.items = into_load(result);
            }
            Msg::CatalogMore(result) => {
                self.catalog.loading_more = false;
                match result {
                    Ok(bundle) => {
                        let next = bundle.next.clone();
                        if matches!(self.catalog.items, Load::Ready(_)) {
                            if let Load::Ready(existing) = &mut self.catalog.items {
                                let mut seen: Vec<u64> =
                                    existing.items.iter().map(|item| item.id).collect();
                                for item in bundle.items {
                                    if !seen.contains(&item.id) {
                                        seen.push(item.id);
                                        existing.items.push(item);
                                    }
                                }
                                existing.icons.extend(bundle.icons);
                                existing.next = next.clone();
                            }
                        } else {
                            self.catalog.items = Load::Ready(bundle);
                        }
                        self.catalog.cursor = next;
                    }
                    Err(error) => self.catalog.more_error = Some(error),
                }
            }
            Msg::CatalogDetail(result) => self.catalog.detail = into_load(result),
            Msg::Profile(result) => {
                if let Ok(bundle) = &result {
                    let id = bundle.user.id;
                    if Some(id) == self.me_id() {
                        self.profile.status_input = bundle.status.clone();
                        self.profile.description_input = bundle.user.description.clone();
                        self.profile.following = Load::Idle;
                    } else {
                        self.load_follow_state(id);
                        self.load_friendship(id);
                    }
                }
                self.profile.data = into_load(result);
            }
            Msg::Avatar(result) => self.avatar.data = into_load(result),
            Msg::Groups(result) => self.groups.data = into_load(result),
            Msg::GroupDetail(result) => self.groups.detail = into_load(result),
            Msg::Inventory(result) => self.inventory.data = into_load(result),
            Msg::Resale(user_asset_id, result) => {
                if let Some(resale) = self.inventory.resale.as_mut() {
                    if resale.user_asset_id == user_asset_id {
                        if let Ok(info) = &result {
                            if resale.price_input.is_empty() {
                                resale.price_input =
                                    info.my_price.map(|price| price.to_string()).unwrap_or_default();
                            }
                        }
                        resale.info = into_load(result);
                    }
                }
            }
            Msg::Transactions(result) => self.money.data = into_load(result),
            Msg::TransactionsMore(result) => {
                self.money.loading_more = false;
                match result {
                    Ok(page) => {
                        if let Load::Ready(existing) = &mut self.money.data {
                            existing.icons.extend(page.icons);
                            existing.items.extend(page.items);
                            existing.next = page.next;
                        } else {
                            self.money.data = Load::Ready(page);
                        }
                    }
                    Err(error) => self.money.more_error = Some(error),
                }
            }
            Msg::Messages(result) => self.messages.data = into_load(result),
            Msg::GameVote(universe, result) => {
                if self.selected_game_id() == Some(universe) {
                    self.games.vote = into_load(result);
                }
            }
            Msg::TradeInventory { owner, result } => {
                let me = self.me_id();
                if let Some(builder) = self.trade_builder.as_mut() {
                    if Some(owner) == me {
                        builder.mine = into_load(result);
                    } else if owner == builder.partner_id {
                        builder.theirs = into_load(result);
                    }
                }
            }
            Msg::UnreadIds(result) => {
                self.messages.preparing_all = false;
                match result {
                    Ok(ids) if ids.is_empty() => {
                        self.notice = Some((true, "Nothing unread in your inbox.".to_string()));
                    }
                    Ok(ids) => self.ask(WriteAction::MarkAllRead { ids }),
                    Err(error) => {
                        self.notice = Some((false, format!("Could not find your unread messages: {error}")));
                    }
                }
            }
            Msg::MessagesMore(result) => {
                self.messages.loading_more = false;
                match result {
                    Ok(page) => {
                        self.messages.page += 1;
                        if let Load::Ready(existing) = &mut self.messages.data {
                            existing.icons.extend(page.icons.clone());
                            for message in page.items {
                                if !existing.items.iter().any(|known| known.id == message.id) {
                                    existing.items.push(message);
                                }
                            }
                            existing.total = existing.total.max(page.total);
                        } else {
                            self.messages.data = Load::Ready(page);
                        }
                    }
                    Err(error) => self.messages.more_error = Some(error),
                }
            }
            Msg::Trades(result) => self.trades.data = into_load(result),
            Msg::TradesMore(result) => {
                self.trades.loading_more = false;
                match result {
                    Ok(page) => {
                        if let Load::Ready(existing) = &mut self.trades.data {
                            for trade in page.items {
                                if !existing.items.iter().any(|known| known.id == trade.id) {
                                    existing.items.push(trade);
                                }
                            }
                            existing.icons.extend(page.icons);
                            existing.next = page.next;
                        } else {
                            self.trades.data = Load::Ready(page);
                        }
                    }
                    Err(error) => self.trades.more_error = Some(error),
                }
            }
            Msg::TradeDetail(id, result) => {
                // Ignore a late answer for a window that has since been closed or
                // switched to another trade.
                if self.trades.selected == Some(id) {
                    self.trades.detail = into_load(result);
                }
            }
            Msg::FriendshipState(id, result) => {
                let showing = self.profile.data.ready().map(|bundle| bundle.user.id);
                if showing == Some(id) || self.profile.user_id == Some(id) {
                    self.profile.friendship = into_load(result);
                }
            }
            Msg::FriendRequests(result) => self.friends.requests = into_load(result),
            Msg::FollowState(id, result) => {
                let showing = self.profile.data.ready().map(|bundle| bundle.user.id);
                if showing == Some(id) || self.profile.user_id == Some(id) {
                    self.profile.following = into_load(result);
                }
            }
            Msg::FavouriteState(id, result) => {
                if self.catalog.selected.as_ref().map(|item| item.id) == Some(id) {
                    self.catalog.favourite = into_load(result);
                }
            }
            Msg::WriteDone(action, result) => self.finish_write(action, result),
            Msg::ChatList(result) => match result {
                Ok(bundle) => {
                    if let Ok(mut gate) = self.notify_gate.lock() {
                        let me = self.session.ready().map(|session| session.user.id).unwrap_or(0);
                        crate::notify::learn(&mut gate, &bundle, me);
                    }
                    self.chat.list = Load::Ready(bundle);
                    if let Some(id) = self.chat.open_conversation() {
                        if self.view == View::Chat {
                            self.mark_chat_read(id);
                        }
                    }
                }
                // A failed background refresh keeps the list that is showing.
                Err(error) => {
                    if self.chat.list.ready().is_none() {
                        self.chat.list = Load::Err(error);
                    }
                }
            },
            Msg::ChatMessages(id, result) => {
                if self.chat.open_conversation() != Some(id) {
                    return;
                }
                match result {
                    Ok(messages) => {
                        let before = self.chat.messages.ready().map(|old| (old.len(), old.last().map(|last| last.id.clone())));
                        let after = (messages.len(), messages.last().map(|last| last.id.clone()));
                        if before.as_ref() != Some(&after) {
                            self.chat.arrivals += 1;
                        }
                        self.chat.messages = Load::Ready(messages);
                        if self.view == View::Chat {
                            self.mark_chat_read(id);
                        }
                    }
                    Err(error) => {
                        if self.chat.messages.ready().is_none() {
                            self.chat.messages = Load::Err(error);
                        }
                    }
                }
            }
            Msg::ChatSent(conversation, message) => {
                self.chat.draft.clear();
                let id = conversation.id;
                if matches!(self.chat.target, Some(ChatTarget::NewWith { .. })) {
                    // The new conversation becomes the open one.
                    self.chat.target = Some(ChatTarget::Conversation(id));
                    self.chat.messages = Load::Ready(Vec::new());
                }
                let mut message = message;
                if message.sender_id == 0 {
                    message.sender_id = self.me_id().unwrap_or(0);
                }
                if message.content.is_empty() {
                    // The echo had no text; the thread refetch below fills it in.
                    self.fetch_chat_messages(id);
                } else if !self.place_chat_message(id, message) {
                    if let Some(me) = self.me_id() {
                        self.refresh_chat_list(me);
                    }
                }
                if let Some(me) = self.me_id() {
                    if self.chat.list.ready().is_some_and(|bundle| !bundle.conversations.iter().any(|item| item.id == id)) {
                        self.refresh_chat_list(me);
                    }
                }
            }
            Msg::Hub(event) => self.on_hub(event),
            Msg::OpenView(view) => {
                self.egui.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                self.egui.send_viewport_cmd(egui::ViewportCommand::Focus);
                if self.session.ready().is_some() {
                    crate::ui::go(self, view);
                    self.reload_current_view();
                }
            }
            Msg::ActivityChanged => self.load_session(),
            Msg::GameServers(place, start, result) => {
                let showing = match &self.games.servers {
                    Load::Ready(page) => Some(page.place_id),
                    _ => None,
                };
                if start == 0 {
                    self.games.servers = into_load(result);
                } else if showing == Some(place) {
                    self.games.servers_more = false;
                    match result {
                        Ok(more) => {
                            if let Load::Ready(page) = &mut self.games.servers {
                                for server in more.servers {
                                    if !page.servers.iter().any(|old| old.job_id == server.job_id) {
                                        page.servers.push(server);
                                    }
                                }
                                page.total = more.total;
                            }
                        }
                        Err(error) => self.notice = Some((false, format!("Could not load more servers: {error}"))),
                    }
                }
            }
            Msg::FriendJoined(user, result) => {
                if self.friends.join.as_ref().map(|(id, _)| *id) == Some(user) {
                    self.friends.join = Some((user, into_load(result)));
                }
            }
            Msg::GameLaunched(place, result) => {
                if self.games.launch.as_ref().map(|(id, _)| *id) == Some(place) {
                    self.games.launch = Some((place, into_load(result)));
                }
            }
            Msg::OpenChat(id) => {
                // Also through egui, in case the native call was refused.
                self.egui.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                self.egui.send_viewport_cmd(egui::ViewportCommand::Focus);
                if self.session.ready().is_some() {
                    self.view = View::Chat;
                    self.on_view_enter();
                    self.open_chat(ChatTarget::Conversation(id));
                }
            }
            Msg::Probe(ok) => self.probe_result = Some(ok),
        }
    }
}

fn into_load<T>(result: Res<T>) -> Load<T> {
    match result {
        Ok(value) => Load::Ready(value),
        Err(error) => Load::Err(error),
    }
}

impl Drop for App {
    fn drop(&mut self) {
        // No icon left behind in the notification area.
        crate::tray::remove_icon();
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.egui = ctx.clone();
        if self.hwnd.load(Ordering::Relaxed) == 0 {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(handle) = frame.window_handle() {
                if let RawWindowHandle::Win32(win32) = handle.as_raw() {
                    self.hwnd.store(win32.hwnd.get(), Ordering::Relaxed);
                    crate::instance::publish(win32.hwnd.get());
                }
            }
        }
        while let Ok(message) = self.rx.try_recv() {
            self.apply(message);
        }

        self.handle_close(ctx);
        if self.start_minimized {
            self.start_minimized = false;
            if self.tray.is_some() {
                // Straight into the tray. eframe shows the window after this
                // first frame and applies this right after, so it never stays up.
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                if let Ok(mut gate) = self.notify_gate.lock() {
                    gate.focused = false;
                }
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
        }
        self.sync_notify_gate(ctx);

        let wanted = format!("{}/{}", self.cfg.theme, self.cfg.accent);
        if self.applied_theme != wanted {
            crate::ui::apply_theme(ctx, &self.cfg.theme, &self.cfg.accent);
            self.applied_theme = wanted;
        }

        if self.session.ready().is_some() {
            self.poll_chat();
            // Keep frames coming while polling, even with no input.
            if self.view == View::Chat {
                ctx.request_repaint_after(Duration::from_secs(1));
            }
        }

        if self.session.ready().is_some() && self.refreshed_at.elapsed() > Duration::from_secs(120) {
            self.refreshed_at = Instant::now();
            self.load_session();
        }

        if self.api.has_session() {
            match self.api.take_auth_status() {
                401 => {
                    self.session = Load::Err("the session cookie was rejected".to_string());
                    self.auth_error = Some(
                        "Your session expired. Paste a fresh .ROBLOSECURITY cookie to continue."
                            .to_string(),
                    );
                }
                403 => {
                    self.stale_error = Some(
                        "a request was forbidden — the session may need refreshing".to_string(),
                    );
                }
                _ => {}
            }
        }

        if ctx.input(|input| input.key_pressed(egui::Key::F5)) {
            self.session = Load::Idle;
            self.load_session();
            self.reload_current_view();
        }

        if self.last_resize_check.elapsed() > Duration::from_secs(2) {
            self.last_resize_check = Instant::now();
            if let Some(rect) = ctx.input(|input| input.viewport().inner_rect) {
                let (width, height) = (rect.width(), rect.height());
                if (width - self.cfg.window_width).abs() >= 8.0
                    || (height - self.cfg.window_height).abs() >= 8.0
                {
                    self.cfg.window_width = width;
                    self.cfg.window_height = height;
                    self.cfg.save();
                }
            }
        }

        let cx = self.cx();
        if !self.api.has_session() || self.auth_error.is_some() {
            crate::ui::login(self, &cx, ctx);
        } else {
            crate::ui::chrome(self, &cx, ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> (TradeSide, TradeSide) {
        let give = TradeSide {
            items: vec![
                ("The Classic HEXIUM Fedora".to_string(), 8_544, None),
                ("Clockwork's Headphones".to_string(), 2_898, Some(12)),
            ],
            rap: 11_442,
            robux: None,
        };
        let get = TradeSide {
            items: vec![("Ghosdeeri".to_string(), 16_500, None)],
            rap: 16_500,
            robux: Some(250),
        };
        (give, get)
    }

    #[test]
    fn accept_summary_lists_both_sides_and_totals() {
        let (give, get) = sample();
        let text = trade_summary("trollbait", true, &give, &get);
        println!("{text}");
        assert!(text.starts_with("Accept trollbait's trade."));
        assert!(text.contains("You give (RAP 11,442)"));
        assert!(text.contains("· The Classic HEXIUM Fedora  (RAP 8,544)"));
        assert!(text.contains("· Clockwork's Headphones  (RAP 2,898)  #12"));
        assert!(text.contains("You get (RAP 16,500)"));
        assert!(text.contains("· plus 250 R$"));
        assert!(text.ends_with("You get 5,058 RAP more than you give."));
    }

    #[test]
    fn resale_summary_warns_below_rap() {
        let text = resale_summary("Beast Mode", Some(108), 1_000, Some(900), None, Some(500));
        println!("{text}");
        assert!(text.starts_with("\"Beast Mode\" #108 goes on sale for 500 R$."));
        assert!(text.contains("RAP: 1,000 R$ (your price is 50% below it)"));
        assert!(text.ends_with("Cheapest on sale now: 900 R$"));
    }

    #[test]
    fn resale_summary_change_and_remove() {
        let change = resale_summary("DJ Money", None, 200, Some(150), Some(300), Some(250));
        assert!(change.starts_with("\"DJ Money\"'s price changes from 300 R$ to 250 R$."));
        assert!(change.ends_with("(yours would cost more)"));
        let remove = resale_summary("DJ Money", None, 200, None, Some(250), None);
        assert!(remove.contains("comes off sale"));
    }

    #[test]
    fn mark_all_read_summary_counts() {
        let text = WriteAction::MarkAllRead { ids: vec![1, 2, 3] }.description();
        assert!(text.starts_with("All 3 unread messages in your inbox"));
        let one = WriteAction::MarkAllRead { ids: vec![7] }.description();
        assert!(one.starts_with("All 1 unread message in your inbox"));
    }

    #[test]
    fn vote_summary_mentions_replacing() {
        let fresh = WriteAction::VoteGame { universe_id: 1, name: "Tower of Hell".into(), up: true, previous: None }.description();
        assert_eq!(fresh, "Your account gives \"Tower of Hell\" a thumbs up.");
        let switch = WriteAction::VoteGame { universe_id: 1, name: "Tower of Hell".into(), up: false, previous: Some(true) }.description();
        assert!(switch.ends_with("This replaces your thumbs up."));
    }

    #[test]
    fn offer_summary_new_and_counter() {
        let (give, get) = sample();
        let text = offer_summary("Larps", false, &give, &get);
        assert!(text.starts_with("Send Larps this trade."));
        assert!(text.contains("You give (RAP 11,442)"));
        assert!(text.contains("You would get 5,058 RAP more than you give."));
        assert!(text.ends_with("Nothing moves until they accept, and they can decline or counter."));
        let counter = offer_summary("Larps", true, &give, &get);
        assert!(counter.starts_with("Send Larps this counter offer. It replaces their offer"));
    }

    #[test]
    fn decline_summary_uses_would_have() {
        let (give, get) = sample();
        let text = trade_summary("trollbait", false, &give, &get);
        assert!(text.starts_with("Decline trollbait's trade."));
        assert!(text.contains("You would have given (RAP 11,442)"));
        assert!(text.contains("You would have got (RAP 16,500)"));
    }
}
