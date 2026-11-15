//! Application state: one screen per message, all network work on a tokio runtime,
//! results drained on the UI thread each frame.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;

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
    Trades,
    Settings,
}

impl View {
    pub const ALL: [View; 12] = [
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
    FavouriteState(u64, Res<bool>),
    WriteDone(WriteAction, Res<()>),
}

/// Something that changes the signed-in account. Nothing here is sent until the
/// user confirms it in the dialog.
#[derive(Debug, Clone)]
pub enum WriteAction {
    Follow { user_id: u64, name: String, follow: bool },
    Favourite { asset_id: u64, name: String, favourite: bool },
    SetStatus { status: String },
    MarkRead { message_id: u64, subject: String, read: bool },
    RequestFriend { user_id: u64, name: String },
    AcceptFriend { user_id: u64, name: String },
    DeclineFriend { user_id: u64, name: String },
    Unfriend { user_id: u64, name: String },
    SendMessage {
        recipient_id: u64,
        recipient: String,
        subject: String,
        body: String,
        reply_to: Option<u64>,
        include_previous: bool,
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
            WriteAction::RequestFriend { .. } => "Send friend request".to_string(),
            WriteAction::AcceptFriend { .. } => "Accept friend request".to_string(),
            WriteAction::DeclineFriend { .. } => "Decline friend request".to_string(),
            WriteAction::Unfriend { .. } => "Unfriend".to_string(),
            WriteAction::SendMessage { reply_to: Some(_), .. } => "Send reply".to_string(),
            WriteAction::SendMessage { .. } => "Send message".to_string(),
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
            WriteAction::RequestFriend { .. } => "Send request",
            WriteAction::AcceptFriend { .. } => "Accept",
            WriteAction::DeclineFriend { .. } => "Decline",
            WriteAction::Unfriend { .. } => "Unfriend",
            WriteAction::SendMessage { .. } => "Send",
        }
    }
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
}

#[derive(Default)]
pub struct GamesState {
    pub sorts: Load<Vec<(String, String)>>,
    pub sort: String,
    pub keyword: String,
    pub games: Load<models::GamesBundle>,
    pub selected: Option<usize>,
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
    refreshed_at: Instant,
    last_resize_check: Instant,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        cfg: Config,
        api: Arc<ApiClient>,
        runtime: tokio::runtime::Runtime,
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
            refreshed_at: Instant::now(),
            last_resize_check: Instant::now(),
        };
        if app.api.has_session() {
            app.load_session();
            app.load_banners();
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
                WriteAction::SendMessage { recipient_id, subject, body, reply_to, include_previous, .. } => {
                    endpoints::send_message(&api, *recipient_id, subject, body, *reply_to, *include_previous).await
                }
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
            WriteAction::SendMessage { recipient, .. } => {
                self.compose = None;
                // A fresh Sent tab should show it.
                if self.messages_tab() == "sent" {
                    self.messages.data = Load::Idle;
                }
                format!("Message sent to {recipient}.")
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
        self.api.set_session(None);
        self.cfg.session_cookie = None;
        self.cfg.save();
        self.session = Load::Idle;
        self.auth_error = None;
        self.stale_error = None;
        self.probe_result = None;
        self.pending_write = None;
        self.compose = None;
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
        self.view = View::Home;
    }

    fn apply(&mut self, message: Msg) {
        match message {
            Msg::Session(result) => match result {
                Ok(session) => {
                    self.session = Load::Ready(session);
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
            Msg::Transactions(result) => self.money.data = into_load(result),
            Msg::TransactionsMore(result) => {
                self.money.loading_more = false;
                match result {
                    Ok(page) => {
                        if let Load::Ready(existing) = &mut self.money.data {
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
            Msg::MessagesMore(result) => {
                self.messages.loading_more = false;
                match result {
                    Ok(page) => {
                        self.messages.page += 1;
                        if let Load::Ready(existing) = &mut self.messages.data {
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

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.egui = ctx.clone();
        while let Ok(message) = self.rx.try_recv() {
            self.apply(message);
        }

        let wanted = format!("{}/{}", self.cfg.theme, self.cfg.accent);
        if self.applied_theme != wanted {
            crate::ui::apply_theme(ctx, &self.cfg.theme, &self.cfg.accent);
            self.applied_theme = wanted;
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
