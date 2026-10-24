//! "Online": signing in to the AnyPlay network, friends, invitations and joining a friend's game.
//! One side panel on the game sheet's frame, with pages that swap in place. The panel only shows
//! and asks; the net service acts, and the host turns the panel's actions into commands for it.

use std::collections::BTreeSet;

use anyps5_core::online::api::{AccountId, Audience, Count, ReportReason};
use anyps5_core::online::{self, Cloud, Link, Online, Pending, Purpose};
use egui::{Color32, Context};

use crate::screens::online_strings::OnlineText as T;
use crate::screens::rows::{self, Change, Row, RowId};
use crate::screens::side::{self, SideState, SideView};
use crate::ui::Shell;
use crate::ui::focus::RowKind;
use crate::ui::i18n::{Arg, Key, Language};
use crate::ui::icons::Icon;
use crate::ui::input::InputEvent;

/// What the panel asks the host to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OnlineAction {
    SetCloud(String),
    /// Back to the cloud AnyPlay runs.
    UseOfficialCloud,
    Email(String),
    Code(String),
    SignUp(String),
    /// Back from the code to the address.
    OtherAddress,
    AddFriend(String),
    Accept(AccountId),
    Decline(AccountId),
    RemoveFriend(AccountId),
    Block(AccountId),
    Unblock(AccountId),
    Report(AccountId, ReportReason),
    /// Join the session a friend plays in.
    Join(AccountId),
    /// Invite a friend to the player's session.
    Invite(AccountId),
    AnswerInvitation {
        id: String,
        accept: bool,
    },
    SignOut,
    DeleteAccount,
    /// The friends picked for the game's invitation dialog, or `None` when it was closed.
    Dialog(Option<Vec<AccountId>>),
    /// Keep every tunnel on a relay, or let them go direct again.
    RelayOnly(bool),
    /// Who sees the player's trophies.
    TrophyAudience(Audience),
    Close,
}

/// The pages of the panel.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Start,
    Cloud,
    Code,
    Handle,
    Home,
    Friend(AccountId),
    Request(AccountId),
    Outgoing(AccountId),
    Blocked(AccountId),
    Invitation(String),
    AddFriend,
    Account,
    ConfirmDelete,
    Report(AccountId),
    Picker,
}

/// What the panel shows, borrowed for one frame.
pub struct OnlineData<'a> {
    pub online: &'a Online,
    /// The cloud in use: the one the player picked, else AnyPlay's own.
    pub cloud: Option<&'a str>,
    /// The player picked a cloud of their own.
    pub custom_cloud: bool,
    /// AnyPlay runs a cloud this build knows.
    pub official_cloud: bool,
    /// Tunnels to other players go only through a relay (`Preferences::online_relay_only`).
    pub relay_only: bool,
    pub language: Language,
    pub accent: Color32,
}

/// The choices of "Who Sees Your Trophies", in the order the row shows them.
const AUDIENCES: [Audience; 3] = [Audience::Friends, Audience::Everyone, Audience::Nobody];

const REASONS: [ReportReason; 5] = [
    ReportReason::Cheating,
    ReportReason::Harassment,
    ReportReason::Spam,
    ReportReason::Name,
    ReportReason::Other,
];

/// The side panel and what is typed into it.
#[derive(Default)]
pub struct OnlinePanel {
    side: SideState,
    pub page: Page,
    pub cloud_text: String,
    pub email: String,
    pub code: String,
    pub handle: String,
    pub age_confirmed: bool,
    pub friend_text: String,
    reason: usize,
    picked: BTreeSet<AccountId>,
    /// The player went back from the code to the address: stay there, though a code is out.
    other_address: bool,
}

impl OnlinePanel {
    /// Opens on the page that fits the state: the game's dialog first, then sign-in or the
    /// friends.
    pub fn open(&mut self, data: &OnlineData<'_>) {
        let page = if data.online.dialog.is_some() && data.online.is_signed_in() {
            Page::Picker
        } else {
            entry_page(data)
        };
        self.go(page, data);
    }

    pub fn close(&mut self) {
        self.side.close();
    }

    pub fn dismiss(&mut self) {
        self.side.dismiss();
    }

    pub fn is_active(&self) -> bool {
        self.side.is_active()
    }

    pub fn is_open(&self) -> bool {
        self.side.is_open()
    }

    /// Shows `page` (previews jump straight to one).
    pub fn show_page(&mut self, page: Page, data: &OnlineData<'_>) {
        self.go(page, data);
    }

    /// Shows `page`, focused on its first row that takes focus.
    fn go(&mut self, page: Page, data: &OnlineData<'_>) {
        if page == Page::Cloud {
            self.cloud_text = data.cloud.unwrap_or_default().to_owned();
        }
        if page == Page::Picker {
            self.picked.clear();
        }
        if matches!(page, Page::Report(_)) {
            self.reason = 0;
        }
        self.page = page;
        let rows = self.rows(data).2;
        let first = rows::kinds(&rows)
            .iter()
            .position(|kind| *kind != RowKind::Info)
            .unwrap_or(0);
        self.side.open_at(first);
    }

    /// Follows the service: a finished sign-in leads to the friends, a sign-out back to the
    /// start, and a page whose friend or invitation is gone back to the list.
    fn follow(&mut self, data: &OnlineData<'_>) {
        let online = data.online;
        let wanted = match &self.page {
            Page::Start | Page::Code | Page::Handle if online.is_signed_in() => Some(Page::Home),
            Page::Start if online.pending == Some(Pending::Code) && !self.other_address => {
                Some(Page::Code)
            }
            Page::Start | Page::Code if online.pending == Some(Pending::Handle) => {
                Some(Page::Handle)
            }
            Page::Code | Page::Handle if online.pending.is_none() && !online.is_signed_in() => {
                Some(Page::Start)
            }
            Page::Cloud | Page::Start | Page::Code | Page::Handle => None,
            _ if !online.is_signed_in() => Some(Page::Start),
            Page::Friend(id) if online.friend(*id).is_none() => Some(Page::Home),
            Page::Request(id) if !online.friends.incoming.iter().any(|r| r.account.id == *id) => {
                Some(Page::Home)
            }
            Page::Outgoing(id) if !online.friends.outgoing.iter().any(|r| r.account.id == *id) => {
                Some(Page::Home)
            }
            Page::Blocked(id) if !online.friends.blocked.iter().any(|a| a.id == *id) => {
                Some(Page::Account)
            }
            Page::Invitation(id) if !online.invitations.iter().any(|i| &i.id == id) => {
                Some(Page::Home)
            }
            Page::Picker if online.dialog.is_none() => Some(Page::Home),
            _ => None,
        };
        if let Some(page) = wanted {
            self.go(page, data);
        }
    }

    /// The page that Back leads to, or `None` to close the panel.
    fn back_page(&self, data: &OnlineData<'_>) -> Option<Page> {
        match self.page {
            Page::Start | Page::Home | Page::Picker => None,
            Page::Cloud => Some(Page::Start),
            Page::Code | Page::Handle => Some(entry_page(data)).filter(|p| *p != self.page),
            Page::Blocked(_) | Page::ConfirmDelete => Some(Page::Account),
            Page::Report(id) => Some(Page::Friend(id)),
            _ => Some(Page::Home),
        }
    }

    pub fn show(
        &mut self,
        shell: &mut Shell,
        ctx: &Context,
        events: &[InputEvent],
        data: &OnlineData<'_>,
    ) -> Vec<OnlineAction> {
        let mut actions = Vec::new();
        if !self.side.is_active() {
            return actions;
        }
        if self.side.is_open() {
            self.follow(data);
            for command in side::commands(events) {
                let rows = self.rows(data).2;
                let driven = rows::drive(shell, &mut self.side.nav, &rows, &[command]);
                for change in &driven.changes {
                    self.handle(ctx, change, data, &mut actions);
                }
                if driven.close {
                    self.back(data, &mut actions);
                    break;
                }
            }
        }
        let (title, subtitle, mut rows) = self.rows(data);
        let view = SideView {
            title: &title,
            subtitle: subtitle.as_deref(),
            list_id: "online-rows",
            nav: &self.side.nav,
            accent: data.accent,
            language: data.language,
            force: None,
        };
        match side::draw(ctx, self.side.is_open(), &view, &mut rows) {
            Some(drawn) if self.side.is_open() => {
                let (changes, texts) =
                    rows::apply_mouse(shell, &mut self.side.nav, &rows, drawn.events);
                for (row, value) in texts {
                    self.typed(row, value);
                }
                for change in &changes {
                    self.handle(ctx, change, data, &mut actions);
                }
                if drawn.scrim_clicked {
                    shell.play(crate::ui::sound::Cue::Back);
                    self.close_panel(data, &mut actions);
                }
            }
            Some(_) => {}
            None => self.side.finished(),
        }
        actions
    }

    fn back(&mut self, data: &OnlineData<'_>, actions: &mut Vec<OnlineAction>) {
        match self.back_page(data) {
            Some(page) => self.go(page, data),
            None => self.close_panel(data, actions),
        }
    }

    fn close_panel(&mut self, data: &OnlineData<'_>, actions: &mut Vec<OnlineAction>) {
        if self.page == Page::Picker && data.online.dialog.is_some() {
            actions.push(OnlineAction::Dialog(None));
        }
        self.side.close();
        actions.push(OnlineAction::Close);
    }

    fn typed(&mut self, row: RowId, value: String) {
        let field = match row {
            "cloud" => &mut self.cloud_text,
            "email" => &mut self.email,
            "code" => &mut self.code,
            "handle" => &mut self.handle,
            "friend" => &mut self.friend_text,
            _ => return,
        };
        *field = value;
    }

    /// The item of a list row: the focused row's place among the rows with the same id.
    fn nth(&self, data: &OnlineData<'_>, id: RowId) -> usize {
        let rows = self.rows(data).2;
        let first = rows::position(&rows, id).unwrap_or(0);
        self.side.nav.row.saturating_sub(first)
    }

    fn handle(
        &mut self,
        ctx: &Context,
        change: &Change,
        data: &OnlineData<'_>,
        actions: &mut Vec<OnlineAction>,
    ) {
        let online = data.online;
        match change {
            Change::Edit { .. } => {
                ctx.data_mut(|d| d.insert_temp(egui::Id::unique("panel-text-focus"), true));
                ctx.request_repaint();
            }
            Change::Toggle { row: "age", on } => self.age_confirmed = *on,
            Change::Toggle {
                row: "relayOnly",
                on,
            } => actions.push(OnlineAction::RelayOnly(*on)),
            Change::Toggle { row: "pick", on } => {
                let index = self.nth(data, "pick");
                if let Some(friend) = picker_friends(online).get(index) {
                    if *on {
                        self.picked.insert(friend.account.id);
                    } else {
                        self.picked.remove(&friend.account.id);
                    }
                }
            }
            Change::Choice {
                row: "reason",
                index,
            } => self.reason = *index,
            Change::Choice {
                row: "trophyAudience",
                index,
            } => {
                if let Some(audience) = AUDIENCES.get(*index) {
                    actions.push(OnlineAction::TrophyAudience(*audience));
                }
            }
            Change::Run { row } => self.run(row, data, actions),
            _ => {}
        }
    }

    fn run(&mut self, row: &str, data: &OnlineData<'_>, actions: &mut Vec<OnlineAction>) {
        let online = data.online;
        match (row, &self.page) {
            ("cloudPage", _) => self.go(Page::Cloud, data),
            ("useCloud", _) if online::is_cloud_address(&self.cloud_text) => {
                actions.push(OnlineAction::SetCloud(self.cloud_text.trim().into()));
                self.go(Page::Start, data);
            }
            ("useOfficial", _) => {
                actions.push(OnlineAction::UseOfficialCloud);
                self.go(Page::Start, data);
            }
            ("sendCode", _) if data.cloud.is_some() => {
                self.other_address = false;
                actions.push(OnlineAction::Email(self.email.trim().into()));
            }
            ("signIn", _) => actions.push(OnlineAction::Code(self.code.trim().into())),
            ("otherAddress", _) => {
                self.code.clear();
                self.other_address = true;
                actions.push(OnlineAction::OtherAddress);
                self.go(Page::Start, data);
            }
            ("create", _) if self.age_confirmed => {
                actions.push(OnlineAction::SignUp(self.handle.trim().into()));
            }
            ("invitation", _) => {
                let index = self.nth(data, "invitation");
                if let Some(invitation) = online.invitations.get(index) {
                    self.go(Page::Invitation(invitation.id.clone()), data);
                }
            }
            ("request", _) => {
                let index = self.nth(data, "request");
                if let Some(request) = online.friends.incoming.get(index) {
                    self.go(Page::Request(request.account.id), data);
                }
            }
            ("friendRow", _) => {
                let index = self.nth(data, "friendRow");
                if let Some(friend) = online.sorted_friends().get(index) {
                    self.go(Page::Friend(friend.account.id), data);
                }
            }
            ("sent", _) => {
                let index = self.nth(data, "sent");
                if let Some(request) = online.friends.outgoing.get(index) {
                    self.go(Page::Outgoing(request.account.id), data);
                }
            }
            ("blockedRow", _) => {
                let index = self.nth(data, "blockedRow");
                if let Some(account) = online.friends.blocked.get(index) {
                    self.go(Page::Blocked(account.id), data);
                }
            }
            ("addFriend", _) => {
                self.friend_text.clear();
                self.go(Page::AddFriend, data);
            }
            ("sendRequest", _) => {
                actions.push(OnlineAction::AddFriend(self.friend_text.trim().into()));
            }
            ("account", _) => self.go(Page::Account, data),
            ("signOut", _) => actions.push(OnlineAction::SignOut),
            ("delete", Page::Account) => self.go(Page::ConfirmDelete, data),
            ("delete", Page::ConfirmDelete) => actions.push(OnlineAction::DeleteAccount),
            ("cancel", Page::ConfirmDelete) => self.go(Page::Account, data),
            ("cancel", Page::Picker) => {
                actions.push(OnlineAction::Dialog(None));
                self.go(Page::Home, data);
            }
            ("accept", Page::Request(id)) => actions.push(OnlineAction::Accept(*id)),
            ("decline", Page::Request(id)) => actions.push(OnlineAction::Decline(*id)),
            ("block", Page::Request(id) | Page::Friend(id)) => {
                actions.push(OnlineAction::Block(*id));
            }
            ("withdraw", Page::Outgoing(id)) => actions.push(OnlineAction::Decline(*id)),
            ("unblock", Page::Blocked(id)) => actions.push(OnlineAction::Unblock(*id)),
            ("join", Page::Friend(id)) => actions.push(OnlineAction::Join(*id)),
            ("invite", Page::Friend(id)) => actions.push(OnlineAction::Invite(*id)),
            ("remove", Page::Friend(id)) => actions.push(OnlineAction::RemoveFriend(*id)),
            ("report", Page::Friend(id)) => {
                let id = *id;
                self.go(Page::Report(id), data);
            }
            ("sendReport", Page::Report(id)) => {
                actions.push(OnlineAction::Report(*id, REASONS[self.reason.min(4)]));
            }
            ("joinInvitation", Page::Invitation(id)) => {
                actions.push(OnlineAction::AnswerInvitation {
                    id: id.clone(),
                    accept: true,
                });
            }
            ("declineInvitation", Page::Invitation(id)) => {
                actions.push(OnlineAction::AnswerInvitation {
                    id: id.clone(),
                    accept: false,
                });
            }
            ("sendInvitations", Page::Picker) if !self.picked.is_empty() => {
                actions.push(OnlineAction::Dialog(Some(
                    self.picked.iter().copied().collect(),
                )));
                self.go(Page::Home, data);
            }
            _ => {}
        }
    }

    /// The page's title, subtitle and rows.
    pub fn rows(&self, data: &OnlineData<'_>) -> (String, Option<String>, Vec<Row>) {
        let language = data.language;
        let t = |text: T| text.text(language);
        let f = |text: T, args: &[Arg<'_>]| text.format(language, args);
        let online = data.online;
        let waiting = |test: fn(&Purpose) -> bool| online.is_waiting(test);
        let mut rows = Vec::new();
        let busy = |row: Row, test: fn(&Purpose) -> bool| {
            if waiting(test) {
                row.detail(t(T::Sending))
            } else {
                row
            }
        };
        let handle_of = |id: AccountId| {
            online
                .known(id)
                .map(|a| a.handle.clone())
                .unwrap_or_default()
        };
        let (title, subtitle) = match &self.page {
            Page::Start => {
                let shown = match data.cloud {
                    Some(_) if !data.custom_cloud => t(T::CloudOfficial),
                    Some(url) => url,
                    None => t(T::CloudNotSet),
                };
                let cloud = Row::action("cloudPage", t(T::Cloud))
                    .icon(Icon::CLOUD)
                    .detail(shown);
                rows.push(cloud);
                rows.push(
                    Row::text("email", t(T::Email), self.email.clone())
                        .icon(Icon::ENVELOPE_SIMPLE)
                        .group_start(),
                );
                let mut send = busy(
                    Row::command("sendCode", t(T::SendCode)).icon(Icon::PAPER_PLANE_RIGHT),
                    |p| matches!(p, Purpose::Email(_) | Purpose::Hello),
                );
                if data.cloud.is_none() {
                    send = send.detail(t(T::NeedsCloud));
                }
                rows.push(send);
                rows.push(Row::info("note", t(T::SignInNote), None).group_start());
                (t(T::Online).to_owned(), Some(t(T::Subtitle).to_owned()))
            }
            Page::Cloud => {
                rows.push(
                    Row::text("cloud", t(T::CloudAddress), self.cloud_text.clone())
                        .icon(Icon::CLOUD),
                );
                let mut use_cloud = Row::command("useCloud", t(T::CloudUse)).icon(Icon::CHECK);
                if !online::is_cloud_address(&self.cloud_text) {
                    use_cloud = use_cloud.detail(t(T::CloudInvalid));
                }
                rows.push(use_cloud);
                if data.custom_cloud && data.official_cloud {
                    rows.push(
                        Row::command("useOfficial", t(T::CloudUseOfficial))
                            .icon(Icon::ARROW_COUNTER_CLOCKWISE)
                            .group_start(),
                    );
                }
                rows.push(Row::info("hint", t(T::CloudHint), None).group_start());
                (t(T::Cloud).to_owned(), Some(t(T::CloudSubtitle).to_owned()))
            }
            Page::Code => {
                rows.push(Row::text("code", t(T::Code), self.code.clone()).icon(Icon::KEY));
                rows.push(busy(
                    Row::command("signIn", t(T::SignIn)).icon(Icon::SIGN_IN),
                    |p| matches!(p, Purpose::Code),
                ));
                rows.push(Row::info("hint", t(T::CodeHint), None));
                rows.push(
                    Row::command("otherAddress", t(T::OtherAddress))
                        .icon(Icon::ARROW_LEFT)
                        .group_start(),
                );
                let to = if online.email.is_empty() {
                    &self.email
                } else {
                    &online.email
                };
                (
                    t(T::Online).to_owned(),
                    Some(f(T::CodeSent, &[Arg::Str(to)])),
                )
            }
            Page::Handle => {
                rows.push(
                    Row::text("handle", t(T::Handle), self.handle.clone()).icon(Icon::USER_CIRCLE),
                );
                rows.push(Row::info("handleHint", t(T::HandleHint), None));
                rows.push(
                    Row::toggle("age", t(T::AgeConfirm), self.age_confirmed)
                        .icon(Icon::CHECK_CIRCLE)
                        .group_start(),
                );
                rows.push(Row::info("ageNote", t(T::AgeNote), None));
                let mut create = busy(
                    Row::command("create", t(T::CreateAccount)).icon(Icon::USER_PLUS),
                    |p| matches!(p, Purpose::SignUp),
                )
                .group_start();
                if !self.age_confirmed {
                    create = create.detail(t(T::AgeNeeded));
                }
                rows.push(create);
                (
                    t(T::NewAccount).to_owned(),
                    Some(t(T::NewAccountSubtitle).to_owned()),
                )
            }
            Page::Home => {
                self.home_rows(data, &mut rows);
                let status = match online.cloud {
                    Some(Cloud::Online) => T::StatusOnline,
                    Some(Cloud::Connecting) | None => T::StatusConnecting,
                    Some(Cloud::Offline) => T::StatusOffline,
                };
                let handle = online
                    .account
                    .as_ref()
                    .map(|a| a.handle.clone())
                    .unwrap_or_default();
                (handle, Some(t(status).to_owned()))
            }
            Page::Friend(id) => {
                let friend = online.friend(*id);
                let presence = friend.map(|f| presence_text(f, language));
                let joinable = friend
                    .and_then(|f| f.presence.activity.as_ref())
                    .and_then(|a| a.session.as_ref())
                    .filter(|s| s.joinable);
                if let Some(session) = joinable {
                    rows.push(
                        Row::command("join", t(T::JoinGame))
                            .icon(Icon::GAME_CONTROLLER)
                            .detail(f(
                                T::JoinDetail,
                                &[
                                    Arg::Int(i64::from(session.members)),
                                    Arg::Int(i64::from(session.max_members)),
                                ],
                            )),
                    );
                }
                let in_session = online
                    .session
                    .as_ref()
                    .is_some_and(|s| !s.members.iter().any(|m| m.account.id == *id));
                if in_session {
                    rows.push(
                        Row::command("invite", t(T::InviteToSession)).icon(Icon::PAPER_PLANE_RIGHT),
                    );
                }
                if let Some(card) = online.cards.get(id) {
                    rows.push(trophy_row(card.as_ref(), language).group_start());
                }
                rows.push(
                    Row::command("remove", t(T::RemoveFriend))
                        .icon(Icon::USER_MINUS)
                        .group_start(),
                );
                rows.push(
                    Row::command("block", t(T::Block))
                        .icon(Icon::PROHIBIT)
                        .destructive(),
                );
                rows.push(Row::action("report", t(T::Report)).icon(Icon::FLAG));
                (handle_of(*id), presence)
            }
            Page::Request(id) => {
                rows.push(Row::command("accept", t(T::Accept)).icon(Icon::CHECK));
                rows.push(Row::command("decline", t(T::Decline)).icon(Icon::X));
                rows.push(
                    Row::command("block", t(T::Block))
                        .icon(Icon::PROHIBIT)
                        .destructive()
                        .group_start(),
                );
                (handle_of(*id), Some(t(T::WantsFriend).to_owned()))
            }
            Page::Outgoing(id) => {
                rows.push(Row::command("withdraw", t(T::Withdraw)).icon(Icon::X));
                (handle_of(*id), Some(t(T::Waiting).to_owned()))
            }
            Page::Blocked(id) => {
                rows.push(Row::command("unblock", t(T::Unblock)).icon(Icon::CHECK));
                (handle_of(*id), Some(t(T::Blocked).to_owned()))
            }
            Page::Invitation(id) => {
                let invitation = online.invitations.iter().find(|i| &i.id == id);
                rows.push(
                    Row::command("joinInvitation", t(T::JoinGame)).icon(Icon::GAME_CONTROLLER),
                );
                rows.push(Row::command("declineInvitation", t(T::Decline)).icon(Icon::X));
                match invitation {
                    Some(invitation) => (
                        f(T::InvitedBy, &[Arg::Str(&invitation.from.handle)]),
                        Some(invitation.title.clone()),
                    ),
                    None => (t(T::Invitations).to_owned(), None),
                }
            }
            Page::AddFriend => {
                rows.push(
                    Row::text("friend", t(T::Handle), self.friend_text.clone())
                        .icon(Icon::USER_CIRCLE),
                );
                rows.push(busy(
                    Row::command("sendRequest", t(T::SendRequest)).icon(Icon::USER_PLUS),
                    |p| matches!(p, Purpose::AddFriend(_)),
                ));
                rows.push(Row::info("hint", t(T::AddFriendHint), None).group_start());
                (
                    t(T::AddFriend).to_owned(),
                    Some(t(T::AddFriendSubtitle).to_owned()),
                )
            }
            Page::Account => {
                let account = online.account.as_ref();
                rows.push(Row::info(
                    "email",
                    t(T::Email),
                    account.map(|a| a.email.clone()),
                ));
                rows.push(Row::info(
                    "cloud",
                    t(T::Cloud),
                    data.cloud.map(str::to_owned),
                ));
                rows.push(
                    Row::toggle("relayOnly", t(T::RelayOnly), data.relay_only)
                        .icon(Icon::EYE_SLASH)
                        .group_start(),
                );
                rows.push(Row::info("relayOnlyNote", t(T::RelayOnlyNote), None));
                let audience = online.privacy.map(|p| p.trophies).unwrap_or_default();
                let audiences = [T::AudienceFriends, T::AudienceEveryone, T::AudienceOnlyYou]
                    .iter()
                    .map(|a| t(*a).to_owned())
                    .collect();
                rows.push(
                    Row::choice(
                        "trophyAudience",
                        t(T::TrophyAudience),
                        audiences,
                        AUDIENCES.iter().position(|a| *a == audience).unwrap_or(0),
                    )
                    .icon(Icon::TROPHY)
                    .group_start(),
                );
                rows.push(Row::info(
                    "trophyAudienceNote",
                    t(T::TrophyAudienceNote),
                    None,
                ));
                if !online.friends.blocked.is_empty() {
                    rows.push(Row::caption("blockedCaption", t(T::Blocked)).group_start());
                    for blocked in &online.friends.blocked {
                        rows.push(
                            Row::action("blockedRow", blocked.handle.clone()).icon(Icon::PROHIBIT),
                        );
                    }
                }
                rows.push(
                    busy(
                        Row::command("signOut", t(T::SignOut)).icon(Icon::SIGN_OUT),
                        |p| matches!(p, Purpose::SignOut),
                    )
                    .group_start(),
                );
                rows.push(
                    Row::action("delete", t(T::DeleteAccount))
                        .icon(Icon::TRASH)
                        .destructive(),
                );
                (t(T::Account).to_owned(), account.map(|a| a.handle.clone()))
            }
            Page::ConfirmDelete => {
                rows.push(busy(
                    Row::command("delete", t(T::DeleteAccount))
                        .icon(Icon::TRASH)
                        .destructive(),
                    |p| matches!(p, Purpose::DeleteAccount),
                ));
                rows.push(Row::command("cancel", language.text(Key::Cancel)).icon(Icon::X));
                (
                    t(T::DeleteAccount).to_owned(),
                    Some(t(T::DeleteAsk).to_owned()),
                )
            }
            Page::Report(id) => {
                let reasons = [
                    T::ReasonCheating,
                    T::ReasonHarassment,
                    T::ReasonSpam,
                    T::ReasonName,
                    T::ReasonOther,
                ]
                .iter()
                .map(|r| t(*r).to_owned())
                .collect();
                rows.push(Row::choice("reason", t(T::Reason), reasons, self.reason));
                rows.push(busy(
                    Row::command("sendReport", t(T::SendReport)).icon(Icon::FLAG),
                    |p| matches!(p, Purpose::Report(_)),
                ));
                (f(T::ReportTitle, &[Arg::Str(&handle_of(*id))]), None)
            }
            Page::Picker => {
                let dialog = online.dialog.as_ref();
                for friend in picker_friends(online) {
                    rows.push(Row::toggle(
                        "pick",
                        friend.account.handle.clone(),
                        self.picked.contains(&friend.account.id),
                    ));
                }
                if rows.is_empty() {
                    rows.push(Row::info("none", t(T::NoFriends), None));
                }
                rows.push(
                    Row::command("sendInvitations", t(T::SendInvitations))
                        .icon(Icon::PAPER_PLANE_RIGHT)
                        .group_start(),
                );
                rows.push(Row::command("cancel", language.text(Key::Cancel)).icon(Icon::X));
                let title = dialog.map(|d| d.title.as_str()).unwrap_or_default();
                let max = dialog.map(|d| i64::from(d.max)).unwrap_or(1);
                (
                    f(T::PickerTitle, &[Arg::Str(title)]),
                    Some(f(T::PickerSubtitle, &[Arg::Int(max)])),
                )
            }
        };
        (title, subtitle, rows)
    }

    fn home_rows(&self, data: &OnlineData<'_>, rows: &mut Vec<Row>) {
        let language = data.language;
        let t = |text: T| text.text(language);
        let online = data.online;
        if !online.invitations.is_empty() {
            rows.push(Row::caption("invitationsCaption", t(T::Invitations)));
            for invitation in &online.invitations {
                rows.push(
                    Row::action(
                        "invitation",
                        T::InvitedBy.format(language, &[Arg::Str(&invitation.from.handle)]),
                    )
                    .icon(Icon::ENVELOPE_SIMPLE)
                    .detail(invitation.title.clone()),
                );
            }
        }
        if !online.friends.incoming.is_empty() {
            rows.push(Row::caption("requestsCaption", t(T::Requests)).group_start());
            for request in &online.friends.incoming {
                rows.push(
                    Row::action("request", request.account.handle.clone())
                        .icon(Icon::USER_PLUS)
                        .detail(t(T::WantsFriend)),
                );
            }
        }
        if let Some(session) = &online.session {
            let me = online.account.as_ref().map(|a| a.id);
            rows.push(Row::caption("sessionCaption", t(T::Session)).group_start());
            for member in &session.members {
                let value = if Some(member.account.id) == me {
                    t(T::You)
                } else {
                    link_text(member.link, language)
                };
                rows.push(Row::info(
                    "member",
                    member.account.handle.clone(),
                    Some(value.to_owned()),
                ));
            }
        }
        rows.push(Row::caption("friendsCaption", t(T::Friends)).group_start());
        let friends = online.sorted_friends();
        if friends.is_empty() {
            rows.push(Row::info("noFriends", t(T::NoFriends), None));
        }
        for friend in friends {
            rows.push(
                Row::action("friendRow", friend.account.handle.clone())
                    .icon(match (&friend.presence.activity, friend.presence.online) {
                        (Some(_), _) => Icon::GAME_CONTROLLER,
                        (None, true) => Icon::USER_CIRCLE,
                        (None, false) => Icon::CIRCLE_DASHED,
                    })
                    .detail(presence_text(friend, language)),
            );
        }
        rows.push(Row::command("addFriend", t(T::AddFriend)).icon(Icon::USER_PLUS));
        if !online.friends.outgoing.is_empty() {
            rows.push(Row::caption("sentCaption", t(T::Sent)).group_start());
            for request in &online.friends.outgoing {
                rows.push(
                    Row::action("sent", request.account.handle.clone())
                        .icon(Icon::HOURGLASS)
                        .detail(t(T::Waiting)),
                );
            }
        }
        rows.push(
            Row::action("account", t(T::Account))
                .icon(Icon::GEAR_SIX)
                .group_start(),
        );
    }
}

/// Sign-in pages come first; signed in, the friends.
fn entry_page(data: &OnlineData<'_>) -> Page {
    let online = data.online;
    if online.is_signed_in() {
        Page::Home
    } else {
        match online.pending {
            Some(Pending::Code) => Page::Code,
            Some(Pending::Handle) => Page::Handle,
            None => Page::Start,
        }
    }
}

/// The friends a game's invitation can go to: those who are online, sorted like the list.
fn picker_friends(online: &Online) -> Vec<&online::api::Friend> {
    online
        .sorted_friends()
        .into_iter()
        .filter(|f| f.presence.online)
        .collect()
}

/// What a friend is doing, in a few words.
/// A friend's trophies: the total, and the grades they have under it, highest first.
fn trophy_row(count: Option<&Count>, language: Language) -> Row {
    let title = language.text(Key::Trophies);
    let Some(count) = count else {
        let hidden = T::TrophiesHidden.text(language).to_owned();
        return Row::info("trophies", title, Some(hidden)).icon(Icon::TROPHY);
    };
    let grades = [
        (count.platinum, Key::GradePlatinum),
        (count.gold, Key::GradeGold),
        (count.silver, Key::GradeSilver),
        (count.bronze, Key::GradeBronze),
    ];
    let total: u32 = grades.iter().map(|(n, _)| n).sum();
    if total == 0 {
        let none = T::TrophiesNone.text(language).to_owned();
        return Row::info("trophies", title, Some(none)).icon(Icon::TROPHY);
    }
    let detail = grades
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, grade)| format!("{n} {}", language.text(*grade)))
        .collect::<Vec<_>>()
        .join(" · ");
    Row::info("trophies", title, Some(total.to_string()))
        .icon(Icon::TROPHY)
        .detail(detail)
}

pub fn presence_text(friend: &online::api::Friend, language: Language) -> String {
    match &friend.presence.activity {
        Some(activity) => {
            let joinable = activity.session.as_ref().is_some_and(|s| s.joinable);
            let text = if joinable {
                T::PlayingJoinable
            } else {
                T::Playing
            };
            text.format(language, &[Arg::Str(&activity.title)])
        }
        None if friend.presence.online => T::FriendOnline.text(language).to_owned(),
        None => T::FriendOffline.text(language).to_owned(),
    }
}

fn link_text(link: Option<Link>, language: Language) -> &'static str {
    match link {
        Some(Link::Direct) => T::LinkDirect.text(language),
        Some(Link::Relay) => T::LinkRelay.text(language),
        _ => T::LinkConnecting.text(language),
    }
}

#[cfg(test)]
mod tests {
    use anyps5_core::online::api::{
        Account, Activity, Friend, FriendLists, Invitation, Presence, Request, SessionRef,
    };
    use anyps5_core::online::{SignedIn, Status};

    use crate::screens::rows::Control;

    use super::*;

    fn account(id: u64, handle: &str) -> Account {
        Account {
            id: AccountId(id),
            handle: handle.into(),
        }
    }

    fn signed_in() -> Online {
        let mut online = Online::default();
        online.status = Status::Running;
        online.cloud = Some(Cloud::Online);
        online.account = Some(SignedIn {
            id: AccountId(1),
            handle: "Alec".into(),
            email: "a@example.org".into(),
        });
        online.friends = FriendLists {
            friends: vec![Friend {
                account: account(2, "Lumen"),
                since: 0,
                presence: Presence {
                    online: true,
                    activity: Some(Activity {
                        title_id: "TEST00001".into(),
                        title: "Test Game".into(),
                        version: "01.000".into(),
                        session: Some(SessionRef {
                            id: "s1".into(),
                            joinable: true,
                            members: 1,
                            max_members: 4,
                        }),
                    }),
                    revision: 0,
                },
            }],
            incoming: vec![Request {
                account: account(3, "Kit"),
                since: 0,
            }],
            ..FriendLists::default()
        };
        online
    }

    fn data(online: &Online) -> OnlineData<'_> {
        OnlineData {
            online,
            cloud: Some("https://cloud.example.org"),
            custom_cloud: true,
            official_cloud: false,
            relay_only: false,
            language: Language::English,
            accent: Color32::WHITE,
        }
    }

    fn ids(rows: &[Row]) -> Vec<&'static str> {
        rows.iter().map(|r| r.id).collect()
    }

    #[test]
    fn before_sign_in_the_panel_asks_for_the_cloud_and_the_address() {
        let online = Online::default();
        let mut panel = OnlinePanel::default();
        panel.open(&data(&online));
        assert_eq!(panel.page, Page::Start);
        let (title, subtitle, rows) = panel.rows(&data(&online));
        assert_eq!(title, "Online");
        assert_eq!(
            subtitle.as_deref(),
            Some("Play with your friends over the internet")
        );
        assert_eq!(ids(&rows), ["cloudPage", "email", "sendCode", "note"]);
        assert_eq!(rows[0].detail.as_deref(), Some("https://cloud.example.org"));
        // Without a cloud the command says what to do first.
        let none = OnlineData {
            cloud: None,
            ..data(&online)
        };
        let rows = panel.rows(&none).2;
        assert_eq!(rows[2].detail.as_deref(), Some("Set up the cloud first."));
    }

    #[test]
    fn the_pages_follow_the_sign_in() {
        let mut online = Online::default();
        let mut panel = OnlinePanel::default();
        panel.open(&data(&online));
        online.pending = Some(Pending::Code);
        panel.follow(&data(&online));
        assert_eq!(panel.page, Page::Code);
        online.pending = Some(Pending::Handle);
        panel.follow(&data(&online));
        assert_eq!(panel.page, Page::Handle);
        let online = signed_in();
        panel.follow(&data(&online));
        assert_eq!(panel.page, Page::Home);
    }

    #[test]
    fn signed_in_the_friends_show_with_what_they_play() {
        let online = signed_in();
        let mut panel = OnlinePanel::default();
        panel.open(&data(&online));
        let (title, subtitle, rows) = panel.rows(&data(&online));
        assert_eq!(title, "Alec");
        assert_eq!(subtitle.as_deref(), Some("Online"));
        assert_eq!(
            ids(&rows),
            [
                "requestsCaption",
                "request",
                "friendsCaption",
                "friendRow",
                "addFriend",
                "account"
            ]
        );
        assert_eq!(
            rows[3].detail.as_deref(),
            Some("Playing Test Game · You can join")
        );
        // The panel opens on the first row that takes focus, not on a caption.
        assert_eq!(panel.side.nav.row, 1);
    }

    #[test]
    fn a_friend_who_plays_in_a_joinable_session_can_be_joined() {
        let online = signed_in();
        let mut panel = OnlinePanel::default();
        panel.go(Page::Friend(AccountId(2)), &data(&online));
        let (title, _, rows) = panel.rows(&data(&online));
        assert_eq!(title, "Lumen");
        assert_eq!(ids(&rows), ["join", "remove", "block", "report"]);
        assert_eq!(rows[0].detail.as_deref(), Some("1 of 4 players"));
        let mut actions = Vec::new();
        panel.run("join", &data(&online), &mut actions);
        assert_eq!(actions, [OnlineAction::Join(AccountId(2))]);
    }

    #[test]
    fn a_friend_shows_their_trophies_or_that_they_hide_them() {
        let mut online = signed_in();
        let mut panel = OnlinePanel::default();
        panel.go(Page::Friend(AccountId(2)), &data(&online));
        assert!(!ids(&panel.rows(&data(&online)).2).contains(&"trophies"));

        let count = |platinum, gold, silver, bronze| Count {
            bronze,
            silver,
            gold,
            platinum,
        };
        online.cards.insert(AccountId(2), Some(count(1, 4, 0, 30)));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(
            ids(&rows),
            ["join", "trophies", "remove", "block", "report"]
        );
        assert_eq!(rows[1].control, Control::Info(Some("35".into())));
        assert_eq!(
            rows[1].detail.as_deref(),
            Some("1 Platinum · 4 Gold · 30 Bronze")
        );
        assert!(rows[1].group_start);

        online.cards.insert(AccountId(2), Some(count(0, 0, 0, 0)));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(rows[1].control, Control::Info(Some("None yet".into())));
        assert_eq!(rows[1].detail, None);

        online.cards.insert(AccountId(2), None);
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(rows[1].control, Control::Info(Some("Hidden".into())));
    }

    #[test]
    fn the_account_page_picks_who_sees_the_trophies() {
        let mut online = signed_in();
        let mut panel = OnlinePanel::default();
        panel.go(Page::Account, &data(&online));
        let rows = panel.rows(&data(&online)).2;
        let choice = rows.iter().position(|r| r.id == "trophyAudience").unwrap();
        assert_eq!(rows[choice + 1].id, "trophyAudienceNote");
        let options = vec!["Friends".to_owned(), "Everyone".into(), "Only You".into()];
        assert_eq!(
            rows[choice].control,
            Control::Choice {
                options: options.clone(),
                selected: 0
            },
            "friends until the cloud says otherwise"
        );
        online.privacy = Some(online::api::Privacy {
            trophies: Audience::Nobody,
        });
        assert_eq!(
            panel.rows(&data(&online)).2[choice].control,
            Control::Choice {
                options,
                selected: 2
            }
        );
        let mut actions = Vec::new();
        let change = Change::Choice {
            row: "trophyAudience",
            index: 1,
        };
        panel.handle(&Context::default(), &change, &data(&online), &mut actions);
        assert_eq!(actions, [OnlineAction::TrophyAudience(Audience::Everyone)]);
    }

    #[test]
    fn a_creation_needs_the_age_confirmation() {
        let mut online = Online::default();
        online.pending = Some(Pending::Handle);
        let mut panel = OnlinePanel::default();
        panel.open(&data(&online));
        panel.handle = "Alec".into();
        let mut actions = Vec::new();
        panel.run("create", &data(&online), &mut actions);
        assert!(actions.is_empty());
        panel.age_confirmed = true;
        panel.run("create", &data(&online), &mut actions);
        assert_eq!(actions, [OnlineAction::SignUp("Alec".into())]);
    }

    #[test]
    fn the_account_page_turns_relay_only_on() {
        let online = signed_in();
        let mut panel = OnlinePanel::default();
        panel.go(Page::Account, &data(&online));
        let rows = panel.rows(&data(&online)).2;
        let toggle = rows.iter().position(|r| r.id == "relayOnly").unwrap();
        assert_eq!(rows[toggle + 1].id, "relayOnlyNote");
        assert_eq!(rows[toggle].title, "Hide Your IP Address");
        let on = OnlineData {
            relay_only: true,
            ..data(&online)
        };
        assert_eq!(
            panel.rows(&on).2[toggle].control,
            crate::screens::rows::Control::Toggle(true)
        );
    }

    #[test]
    fn deleting_the_account_asks_first_and_opens_on_the_question() {
        let online = signed_in();
        let mut panel = OnlinePanel::default();
        panel.go(Page::Account, &data(&online));
        let mut actions = Vec::new();
        panel.run("delete", &data(&online), &mut actions);
        assert!(actions.is_empty());
        assert_eq!(panel.page, Page::ConfirmDelete);
        let rows = panel.rows(&data(&online)).2;
        assert!(rows[0].destructive);
        panel.run("delete", &data(&online), &mut actions);
        assert_eq!(actions, [OnlineAction::DeleteAccount]);
    }

    #[test]
    fn back_leads_up_the_pages_and_closes_at_the_top() {
        let online = signed_in();
        let mut panel = OnlinePanel::default();
        panel.go(Page::Report(AccountId(2)), &data(&online));
        assert_eq!(
            panel.back_page(&data(&online)),
            Some(Page::Friend(AccountId(2)))
        );
        panel.go(Page::Friend(AccountId(2)), &data(&online));
        assert_eq!(panel.back_page(&data(&online)), Some(Page::Home));
        panel.go(Page::Home, &data(&online));
        assert_eq!(panel.back_page(&data(&online)), None);
    }

    #[test]
    fn an_invitation_page_answers_it() {
        let mut online = signed_in();
        online.invitations.push(Invitation {
            id: "i1".into(),
            from: account(2, "Lumen"),
            session: "s1".into(),
            title_id: "TEST00001".into(),
            title: "Test Game".into(),
            version: "01.000".into(),
            expires: 0,
        });
        let mut panel = OnlinePanel::default();
        panel.go(Page::Invitation("i1".into()), &data(&online));
        let (title, subtitle, _) = panel.rows(&data(&online));
        assert_eq!(title, "Lumen invites you");
        assert_eq!(subtitle.as_deref(), Some("Test Game"));
        let mut actions = Vec::new();
        panel.run("joinInvitation", &data(&online), &mut actions);
        assert_eq!(
            actions,
            [OnlineAction::AnswerInvitation {
                id: "i1".into(),
                accept: true
            }]
        );
        // Once it is gone, the page returns to the list.
        online.invitations.clear();
        panel.follow(&data(&online));
        assert_eq!(panel.page, Page::Home);
    }
}
