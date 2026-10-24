//! The launcher's side of the AnyPlay network.
//!
//! The net service (`anyplay-net`, from anyplay-gg/network) is a program of its own that holds the
//! account, talks to the cloud and carries game traffic. The launcher starts it, speaks the
//! control protocol with it over its standard streams (`spec/control-protocol.md` there) and shows
//! what it reports. Two parts:
//!
//! - [`NetProcess`] runs the child process: one reader thread, writes that never wait on the
//!   service, and a stop that closes the input so the service can leave its sessions.
//! - [`Online`] is what the launcher knows: sign-in, friends, invitations, the player's session.
//!   It is plain data, updated from the service's output, and keeps track of what each command it
//!   sent was for, so the UI can tell the player how it went.
//!
//! Nothing here goes online by itself: the launcher starts the process only once the player asks
//! to sign in, or when they signed in on this PC before.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command as Process, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

pub use anyplay_control::anyplay_api as api;
pub use anyplay_control::{
    self as control, Cloud, Command, Event, Failure, Link, Output, Pending, Session, SignedIn,
};
use api::{Account, AccountId, FriendLists, Invitation};

use crate::events::Notifier;
use crate::trophies::{Earned, Grade};

/// The program name of the net service.
pub const BINARY: &str = if cfg!(windows) {
    "anyplay-net.exe"
} else {
    "anyplay-net"
};

/// The environment variable that names the net service to run instead of the bundled one.
pub const BINARY_VARIABLE: &str = "ANYPLAY_NET";

/// AnyPlay's own cloud, the one players sign in to unless they pick another. A release build
/// names it with `ANYPLAY_DEFAULT_CLOUD`; without it the player has to enter a cloud.
pub const DEFAULT_CLOUD: Option<&str> = option_env!("ANYPLAY_DEFAULT_CLOUD");

/// The cloud the player uses: the one they picked, else AnyPlay's own.
pub fn cloud(picked: Option<&str>) -> Option<&str> {
    picked.or(DEFAULT_CLOUD).filter(|url| !url.is_empty())
}

/// True for an address a cloud can have: https, or http on this PC for a cloud under test.
pub fn is_cloud_address(text: &str) -> bool {
    let text = text.trim();
    let rest = if let Some(rest) = text.strip_prefix("https://") {
        rest
    } else if let Some(rest) = text.strip_prefix("http://") {
        let host = match rest.strip_prefix('[') {
            Some(v6) => v6.split(']').next().unwrap_or_default(),
            None => rest.split(['/', ':']).next().unwrap_or_default(),
        };
        if !matches!(host, "localhost" | "127.0.0.1" | "::1") {
            return false;
        }
        rest
    } else {
        return false;
    };
    !rest.is_empty()
        && text.len() <= 2048
        && !text.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// A line of output, or the end of the process.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    Output(Output),
    Ended,
}

/// The net service as a child process.
pub struct NetProcess {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    lines: Receiver<Line>,
}

impl NetProcess {
    /// Where the net service is: `ANYPLAY_NET`, else next to the launcher's own executable.
    pub fn locate() -> Option<PathBuf> {
        if let Some(path) = std::env::var_os(BINARY_VARIABLE).map(PathBuf::from) {
            return path.is_file().then_some(path);
        }
        let exe = std::env::current_exe().ok()?;
        let path = exe.parent()?.join(BINARY);
        path.is_file().then_some(path)
    }

    /// Starts `binary` with its files in `data`, keeping every tunnel on a relay when
    /// `relay_only`; its log goes to the end of `log`. `notify` is called after every line it
    /// writes.
    pub fn spawn(
        binary: &Path,
        data: &Path,
        relay_only: bool,
        log: &Path,
        notify: Notifier,
    ) -> std::io::Result<Self> {
        std::fs::create_dir_all(data)?;
        if let Some(parent) = log.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)?;
        let mut command = Process::new(binary);
        command
            .arg("--data")
            .arg(data)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(log);
        if relay_only {
            command.arg("--relay-only");
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = command.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("no output pipe"))?;
        let stdin = child.stdin.take();
        let (tx, lines) = channel();
        std::thread::Builder::new()
            .name("net-read".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                let mut line = Vec::new();
                loop {
                    line.clear();
                    let read = (&mut reader)
                        .take(control::MAX_LINE as u64)
                        .read_until(b'\n', &mut line);
                    match read {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    let Ok(text) = std::str::from_utf8(&line) else {
                        continue;
                    };
                    if let Some(output) = control::parse_output(text.trim_end()) {
                        if tx.send(Line::Output(output)).is_err() {
                            return;
                        }
                        notify();
                    }
                }
                let _ = tx.send(Line::Ended);
                notify();
            })?;
        Ok(Self {
            child: Some(child),
            stdin,
            lines,
        })
    }

    /// Writes command lines. An error means the service is gone; its end arrives as a line.
    pub fn write(&mut self, lines: &[String]) -> std::io::Result<()> {
        let Some(stdin) = &mut self.stdin else {
            return Err(std::io::ErrorKind::BrokenPipe.into());
        };
        for line in lines {
            stdin.write_all(line.as_bytes())?;
        }
        stdin.flush()
    }

    /// The lines that arrived since the last call.
    pub fn drain(&self) -> Vec<Line> {
        self.lines.try_iter().collect()
    }
}

impl Drop for NetProcess {
    /// Closes the service's input. It leaves its sessions and ends by itself (or, with a game
    /// still connected, once that game ends); a service that does not end in ten seconds while
    /// no game holds it is stopped.
    fn drop(&mut self) {
        drop(self.stdin.take());
        if let Some(mut child) = self.child.take() {
            let _ = std::thread::Builder::new()
                .name("net-stop".into())
                .spawn(move || {
                    for _ in 0..100 {
                        if matches!(child.try_wait(), Ok(Some(_)) | Err(_)) {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    tracing::info!("the net service is still running; a game may hold it");
                });
        }
    }
}

/// Whether the net service runs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Status {
    /// Not started: the player has not signed in on this PC.
    #[default]
    Off,
    Running,
    /// This installation has no net service.
    Missing,
    /// It could not start or it ended; the reason is for the log.
    Ended(String),
}

/// A join the launcher checks before it starts the game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Join {
    pub session: String,
    pub title_id: String,
    pub title: String,
    pub version: String,
    pub build: String,
    /// The friend whose session it is, for the messages.
    pub friend: String,
}

/// What a command was sent for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Purpose {
    Hello,
    Email(String),
    Code,
    SignUp,
    SignOut,
    DeleteAccount,
    AddFriend(String),
    Accept(AccountId),
    Decline(AccountId),
    RemoveFriend(AccountId),
    Block(AccountId),
    Unblock(AccountId),
    Report(AccountId),
    Playing,
    CheckJoin(Join),
    /// A game start; the number pairs it with the launcher's start.
    GameStart(u64),
    GameEnd,
    Invite,
    AnswerInvitation {
        id: String,
        accept: bool,
    },
    Dialog,
    RelayOnly,
    /// The records of a title; `count` is how many the player has earned in all.
    UploadTrophies {
        title_id: String,
        count: usize,
    },
    Cards,
    Privacy,
    SetPrivacy,
}

/// The game's invitation dialog, waiting for the player to pick friends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dialog {
    pub id: u64,
    pub title: String,
    pub max: u16,
}

/// Something the launcher may want to tell the player about.
#[derive(Clone, Debug, PartialEq)]
pub enum Notice {
    /// A command came back.
    Answered(Purpose, Result<serde_json::Value, Failure>),
    /// Somebody asked to be friends.
    Request(Account),
    /// A friend invited the player.
    Invitation(Invitation),
    /// The running game opened its invitation dialog.
    Dialog(Dialog),
    /// The net service ended.
    Ended,
}

/// What the launcher knows about the player's online state.
#[derive(Debug, Default)]
pub struct Online {
    pub status: Status,
    pub cloud: Option<Cloud>,
    pub account: Option<SignedIn>,
    pub pending: Option<Pending>,
    /// The address the last code went to.
    pub email: String,
    pub friends: FriendLists,
    pub invitations: Vec<Invitation>,
    pub session: Option<Session>,
    pub dialog: Option<Dialog>,
    /// Friends' trophy counts by grade; `None` when they hide them from the player.
    pub cards: BTreeMap<AccountId, Option<api::Count>>,
    /// Who sees the player's trophies; `None` until the cloud said.
    pub privacy: Option<api::Privacy>,
    /// How many trophies of each title the cloud has recorded since this sign-in.
    uploaded: BTreeMap<String, usize>,
    waiting: BTreeMap<u64, Purpose>,
    next: u64,
    outbox: Vec<String>,
}

impl Online {
    /// Queues `command`; [`Online::take_outbox`] hands it to the process.
    pub fn send(&mut self, purpose: Purpose, command: Command) -> u64 {
        self.next += 1;
        let id = self.next;
        self.outbox.push(control::command_line(id, &command));
        self.waiting.insert(id, purpose);
        id
    }

    pub fn take_outbox(&mut self) -> Vec<String> {
        std::mem::take(&mut self.outbox)
    }

    /// True while a command for which `test` holds waits for its answer.
    pub fn is_waiting(&self, test: impl Fn(&Purpose) -> bool) -> bool {
        self.waiting.values().any(test)
    }

    /// The player is signed in and the cloud answers.
    pub fn is_online(&self) -> bool {
        self.account.is_some() && self.cloud == Some(Cloud::Online)
    }

    pub fn is_signed_in(&self) -> bool {
        self.account.is_some()
    }

    /// Forgets the process and its state, keeping nothing of the account.
    pub fn stopped(&mut self, status: Status) -> Vec<Notice> {
        let notices = self
            .waiting
            .values()
            .map(|purpose| {
                Notice::Answered(
                    purpose.clone(),
                    Err(Failure::new(Failure::OFFLINE, "The net service ended.")),
                )
            })
            .collect();
        *self = Online {
            status,
            next: self.next,
            ..Online::default()
        };
        notices
    }

    /// Applies one line of the service's output.
    pub fn apply(&mut self, line: Line) -> Vec<Notice> {
        match line {
            Line::Ended => {
                let mut notices = self.stopped(Status::Ended("the net service ended".into()));
                notices.push(Notice::Ended);
                notices
            }
            Line::Output(Output::Answer(answer)) => {
                let Some(purpose) = self.waiting.remove(&answer.id) else {
                    return Vec::new();
                };
                let result = answer.into_result::<serde_json::Value>();
                if let Ok(value) = &result {
                    self.answered(&purpose, value);
                }
                vec![Notice::Answered(purpose, result)]
            }
            Line::Output(Output::Event(event)) => self.event(event),
        }
    }

    /// Keeps what a successful answer tells about the account.
    fn answered(&mut self, purpose: &Purpose, value: &serde_json::Value) {
        match purpose {
            Purpose::Email(email) => self.email.clone_from(email),
            Purpose::UploadTrophies { title_id, count } => {
                let known = self.uploaded.entry(title_id.clone()).or_default();
                *known = (*known).max(*count);
            }
            Purpose::Cards => {
                if let Ok(cards) = serde_json::from_value::<api::Cards>(value.clone()) {
                    for card in cards.cards {
                        self.cards.insert(card.account.id, card.trophies);
                    }
                }
            }
            Purpose::Privacy | Purpose::SetPrivacy => {
                if let Ok(privacy) = serde_json::from_value(value.clone()) {
                    self.privacy = Some(privacy);
                }
            }
            _ => {}
        }
    }

    /// Sends the trophies the player earned in a title, unless the cloud has them all already or
    /// an upload of the title still waits. The cloud keeps the first time of each trophy, so a
    /// repeat is harmless; this only saves the request.
    pub fn upload_trophies(&mut self, title_id: &str, earned: &[Earned]) {
        let count = earned.len();
        let sending =
            |p: &Purpose| matches!(p, Purpose::UploadTrophies { title_id: t, .. } if t == title_id);
        if !self.is_online()
            || count == 0
            || !api::is_title_id(title_id)
            || self
                .uploaded
                .get(title_id)
                .is_some_and(|&known| known >= count)
            || self.is_waiting(sending)
        {
            return;
        }
        let trophies: Vec<api::Trophy> = earned
            .iter()
            .filter_map(|e| {
                Some(api::Trophy {
                    number: u16::try_from(e.number)
                        .ok()
                        .filter(|n| *n < api::MAX_TROPHY_NUMBER)?,
                    grade: match e.grade {
                        Grade::Platinum => api::Grade::Platinum,
                        Grade::Gold => api::Grade::Gold,
                        Grade::Silver => api::Grade::Silver,
                        Grade::Bronze => api::Grade::Bronze,
                    },
                    earned: u64::try_from(e.unlocked.div_euclid(1_000_000)).unwrap_or(0),
                })
            })
            .collect();
        let chunks = trophies.chunks(api::MAX_TROPHY_UPLOAD).count();
        for (index, chunk) in trophies.chunks(api::MAX_TROPHY_UPLOAD).enumerate() {
            // Only the last part's answer says that the cloud has them all.
            let count = if index + 1 == chunks { count } else { 0 };
            self.send(
                Purpose::UploadTrophies {
                    title_id: title_id.to_owned(),
                    count,
                },
                Command::UploadTrophies {
                    title_id: title_id.to_owned(),
                    trophies: chunk.to_vec(),
                },
            );
        }
    }

    /// Asks for the gamer cards of the friends: all of them, or only those without one yet.
    pub fn request_cards(&mut self, all: bool) {
        if !self.is_online() || self.is_waiting(|p| *p == Purpose::Cards) {
            return;
        }
        let accounts: Vec<AccountId> = self
            .friends
            .friends
            .iter()
            .map(|f| f.account.id)
            .filter(|id| all || !self.cards.contains_key(id))
            .collect();
        for chunk in accounts.chunks(api::MAX_CARDS) {
            self.send(
                Purpose::Cards,
                Command::Cards {
                    accounts: chunk.to_vec(),
                },
            );
        }
    }

    /// Changes who sees the player's trophies. The cloud replaces the whole setting, so the other
    /// parts go back as the cloud last said; before it said anything, the launcher asks again
    /// instead of guessing them. The choice shows at once and comes back with the answer.
    pub fn set_trophy_audience(&mut self, trophies: api::Audience) {
        if !self.is_signed_in() {
            return;
        }
        let Some(mut privacy) = self.privacy else {
            self.send(Purpose::Privacy, Command::Privacy);
            return;
        };
        privacy.trophies = trophies;
        self.privacy = Some(privacy);
        self.send(Purpose::SetPrivacy, Command::SetPrivacy { privacy });
    }

    fn event(&mut self, event: Event) -> Vec<Notice> {
        let mut notices = Vec::new();
        match event {
            Event::State {
                cloud,
                account,
                pending,
            } => {
                let was_online = self.is_online();
                self.cloud = Some(cloud);
                self.account = account;
                self.pending = pending;
                if self.account.is_none() {
                    self.friends = FriendLists::default();
                    self.invitations.clear();
                    self.session = None;
                    self.cards.clear();
                    self.privacy = None;
                    self.uploaded.clear();
                }
                if !was_online && self.is_online() {
                    self.send(Purpose::Privacy, Command::Privacy);
                    self.request_cards(true);
                }
            }
            Event::Friends(lists) => {
                for request in &lists.incoming {
                    let known = self
                        .friends
                        .incoming
                        .iter()
                        .any(|r| r.account.id == request.account.id);
                    if !known && self.account.is_some() {
                        notices.push(Notice::Request(request.account.clone()));
                    }
                }
                self.friends = lists;
                self.request_cards(false);
            }
            Event::Invitation { invitation } => {
                self.invitations.retain(|i| i.id != invitation.id);
                self.invitations.push(invitation.clone());
                notices.push(Notice::Invitation(invitation));
            }
            Event::InvitationGone { id } => self.invitations.retain(|i| i.id != id),
            Event::Session { session } => self.session = session,
            Event::InviteDialog { id, title, max } => {
                let dialog = Dialog { id, title, max };
                self.dialog = Some(dialog.clone());
                notices.push(Notice::Dialog(dialog));
            }
            Event::Notice { code, message } => {
                tracing::info!("net service: {code}: {message}");
            }
            // Parties and messages get their own screens; until then the launcher leaves them.
            Event::Party { .. }
            | Event::PartyInvitation { .. }
            | Event::PartyInvitationGone { .. }
            | Event::Conversations(_)
            | Event::Message { .. }
            | Event::MessageRequest { .. }
            | Event::Read { .. }
            | Event::Conversation { .. }
            | Event::ConversationGone { .. }
            | Event::Unknown => {}
        }
        notices
    }

    /// Friends sorted for the list: those whose game can be joined, then those who play, then the
    /// others online, then the rest; by name within each.
    pub fn sorted_friends(&self) -> Vec<&api::Friend> {
        let mut friends: Vec<&api::Friend> = self.friends.friends.iter().collect();
        friends.sort_by_key(|f| {
            let joinable = f
                .presence
                .activity
                .as_ref()
                .and_then(|a| a.session.as_ref())
                .is_some_and(|s| s.joinable);
            let rank = match (&f.presence.activity, f.presence.online) {
                (Some(_), _) if joinable => 0,
                (Some(_), _) => 1,
                (None, true) => 2,
                (None, false) => 3,
            };
            (rank, f.account.handle.to_lowercase())
        });
        friends
    }

    /// The friend with this account, if they are one.
    pub fn friend(&self, account: AccountId) -> Option<&api::Friend> {
        self.friends
            .friends
            .iter()
            .find(|f| f.account.id == account)
    }

    /// The account behind an incoming request, an outgoing one or a block.
    pub fn known(&self, account: AccountId) -> Option<&Account> {
        self.friends
            .friends
            .iter()
            .map(|f| &f.account)
            .chain(self.friends.incoming.iter().map(|r| &r.account))
            .chain(self.friends.outgoing.iter().map(|r| &r.account))
            .chain(self.friends.blocked.iter())
            .find(|a| a.id == account)
    }
}

/// The variables a game gets beyond its own settings: the bridge's from the net service, and
/// the observation log when the player records network calls.
pub fn game_environment(
    bridge: Option<&BTreeMap<String, String>>,
    record: Option<&Path>,
) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    if let Some(bridge) = bridge {
        for (name, value) in bridge {
            // Only the bridge's own variables, whatever else an answer might carry.
            if name.starts_with("ANYPS5_NET_") {
                env.insert(name.clone(), value.clone());
            }
        }
    }
    if let Some(record) = record {
        env.insert(
            "ANYPS5_NET_OBSERVE".to_owned(),
            record.to_string_lossy().into_owned(),
        );
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;
    use api::{Activity, Friend, Presence, Request};

    fn account(id: u64, handle: &str) -> Account {
        Account {
            id: AccountId(id),
            handle: handle.into(),
        }
    }

    fn event(event: Event) -> Line {
        Line::Output(Output::Event(event))
    }

    fn signed_in(online: &mut Online) {
        online.apply(event(Event::State {
            cloud: Cloud::Online,
            account: Some(SignedIn {
                id: AccountId(1),
                handle: "Alec".into(),
                email: "a@example.org".into(),
            }),
            pending: None,
        }));
    }

    #[test]
    fn answers_come_back_with_what_they_were_for() {
        let mut online = Online::default();
        let id = online.send(
            Purpose::Email("a@example.org".into()),
            Command::Email {
                email: "a@example.org".into(),
            },
        );
        let lines = online.take_outbox();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("\"do\":\"email\""));
        assert!(online.is_waiting(|p| matches!(p, Purpose::Email(_))));
        let notices = online.apply(Line::Output(Output::Answer(control::Answer::ok(
            id,
            control::EmailSent { code_length: 6 },
        ))));
        assert!(matches!(
            &notices[..],
            [Notice::Answered(Purpose::Email(_), Ok(_))]
        ));
        assert_eq!(online.email, "a@example.org");
        assert!(!online.is_waiting(|_| true));
        // A stray answer is ignored.
        assert!(
            online
                .apply(Line::Output(Output::Answer(control::Answer::ok(99, ()))))
                .is_empty()
        );
    }

    #[test]
    fn a_new_request_is_announced_once() {
        let mut online = Online::default();
        signed_in(&mut online);
        let lists = FriendLists {
            incoming: vec![Request {
                account: account(2, "Lumen"),
                since: 1,
            }],
            ..FriendLists::default()
        };
        let first = online.apply(event(Event::Friends(lists.clone())));
        assert_eq!(first, vec![Notice::Request(account(2, "Lumen"))]);
        assert!(online.apply(event(Event::Friends(lists))).is_empty());
    }

    #[test]
    fn signing_out_forgets_the_lists() {
        let mut online = Online::default();
        signed_in(&mut online);
        online.apply(event(Event::Friends(FriendLists {
            friends: vec![Friend {
                account: account(2, "Lumen"),
                since: 1,
                presence: Presence::default(),
            }],
            ..FriendLists::default()
        })));
        assert!(online.is_online());
        online.apply(event(Event::State {
            cloud: Cloud::Online,
            account: None,
            pending: None,
        }));
        assert!(online.friends.friends.is_empty());
        assert!(!online.is_signed_in());
    }

    #[test]
    fn the_end_of_the_service_fails_what_waits() {
        let mut online = Online {
            status: Status::Running,
            ..Online::default()
        };
        online.send(Purpose::SignOut, Command::SignOut);
        let notices = online.apply(Line::Ended);
        assert!(matches!(
            &notices[0],
            Notice::Answered(Purpose::SignOut, Err(f)) if f.code == Failure::OFFLINE
        ));
        assert_eq!(notices.last(), Some(&Notice::Ended));
        assert!(matches!(online.status, Status::Ended(_)));
        // Numbers keep counting, so a late answer of the old process never matches a new command.
        assert_eq!(online.send(Purpose::Hello, Command::SignOut), 2);
    }

    fn answer(online: &mut Online, id: u64, value: impl serde::Serialize) {
        online.apply(Line::Output(Output::Answer(control::Answer::ok(id, value))));
    }

    fn earned(number: u32, grade: Grade, unlocked: i64) -> Earned {
        Earned {
            number,
            grade,
            unlocked,
        }
    }

    #[test]
    fn signing_in_asks_for_the_privacy_choice() {
        let mut online = Online::default();
        signed_in(&mut online);
        let lines = online.take_outbox();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("\"do\":\"privacy\""));
        answer(
            &mut online,
            1,
            api::Privacy {
                trophies: api::Audience::Everyone,
                ..api::Privacy::default()
            },
        );
        assert_eq!(
            online.privacy.map(|p| p.trophies),
            Some(api::Audience::Everyone)
        );
    }

    #[test]
    fn the_trophy_choice_keeps_the_other_parts_of_the_privacy() {
        let mut online = Online::default();
        signed_in(&mut online);
        online.take_outbox();
        online.set_trophy_audience(api::Audience::Nobody);
        let lines = online.take_outbox();
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].contains("\"do\":\"privacy\""),
            "it asks before it knows the rest"
        );
        let known = api::Privacy {
            about: api::Audience::Everyone,
            ..api::Privacy::default()
        };
        answer(&mut online, 1, known);
        online.set_trophy_audience(api::Audience::Nobody);
        let (_, command) = control::parse_command(&online.take_outbox()[0]).unwrap();
        assert_eq!(
            command,
            Command::SetPrivacy {
                privacy: api::Privacy {
                    trophies: api::Audience::Nobody,
                    ..known
                },
            }
        );
    }

    #[test]
    fn trophies_go_up_once_for_each_new_count() {
        let mut online = Online::default();
        let first = [
            earned(0, Grade::Platinum, 3_500_000),
            earned(7, Grade::Silver, 1),
        ];
        online.upload_trophies("GAME01234", &first);
        assert!(
            online.take_outbox().is_empty(),
            "nothing goes up signed out"
        );
        signed_in(&mut online);
        online.take_outbox();

        online.upload_trophies("GAME01234", &first);
        online.upload_trophies("GAME01234", &first);
        let lines = online.take_outbox();
        assert_eq!(lines.len(), 1, "a second call waits for the first");
        let (_, command) = control::parse_command(&lines[0]).unwrap();
        assert_eq!(
            command,
            Command::UploadTrophies {
                title_id: "GAME01234".into(),
                trophies: vec![
                    api::Trophy {
                        number: 0,
                        grade: api::Grade::Platinum,
                        earned: 3,
                    },
                    api::Trophy {
                        number: 7,
                        grade: api::Grade::Silver,
                        earned: 0,
                    },
                ],
            }
        );
        answer(&mut online, 2, api::TrophiesAdded { added: 2 });
        online.upload_trophies("GAME01234", &first);
        assert!(online.take_outbox().is_empty(), "the cloud has these");
        online.upload_trophies(
            "GAME01234",
            &[first[0], first[1], earned(9, Grade::Bronze, 1)],
        );
        assert_eq!(online.take_outbox().len(), 1);
        online.upload_trophies("../admin", &first);
        assert!(
            online.take_outbox().is_empty(),
            "a title ID goes into a path"
        );
    }

    #[test]
    fn friends_get_their_cards_once_and_hidden_ones_stay_none() {
        let mut online = Online::default();
        signed_in(&mut online);
        online.take_outbox();
        let friend = |id, handle: &str| Friend {
            account: account(id, handle),
            since: 0,
            presence: Presence::default(),
        };
        let lists = FriendLists {
            friends: vec![friend(2, "Lumen"), friend(3, "Bea")],
            ..FriendLists::default()
        };
        online.apply(event(Event::Friends(lists.clone())));
        let lines = online.take_outbox();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("\"do\":\"cards\""));
        let count = api::Count {
            bronze: 3,
            silver: 2,
            gold: 1,
            platinum: 0,
        };
        answer(
            &mut online,
            2,
            api::Cards {
                cards: vec![
                    api::Card {
                        account: account(2, "Lumen"),
                        trophies: Some(count),
                        avatar: None,
                    },
                    api::Card {
                        account: account(3, "Bea"),
                        trophies: None,
                        avatar: None,
                    },
                ],
            },
        );
        assert_eq!(online.cards.get(&AccountId(2)), Some(&Some(count)));
        assert_eq!(online.cards.get(&AccountId(3)), Some(&None));
        online.apply(event(Event::Friends(lists)));
        assert!(online.take_outbox().is_empty(), "every friend has a card");
        online.request_cards(true);
        assert_eq!(online.take_outbox().len(), 1, "a refresh asks for all");
    }

    #[test]
    fn friends_who_play_come_first() {
        use api::SessionRef;
        let mut online = Online::default();
        let friend = |id, handle: &str, online: bool, playing: bool, joinable: bool| Friend {
            account: account(id, handle),
            since: 0,
            presence: Presence {
                online,
                activity: playing.then(|| Activity {
                    title_id: "TEST00001".into(),
                    title: "Test".into(),
                    version: "01.000".into(),
                    session: joinable.then(|| SessionRef {
                        id: "s".into(),
                        joinable: true,
                        members: 1,
                        max_members: 4,
                    }),
                }),
                revision: 0,
            },
        };
        online.friends.friends = vec![
            friend(2, "zed", false, false, false),
            friend(3, "Bea", true, false, false),
            friend(4, "amy", true, true, false),
            friend(5, "Abe", false, false, false),
            friend(6, "Zoe", true, true, true),
        ];
        let order: Vec<&str> = online
            .sorted_friends()
            .iter()
            .map(|f| f.account.handle.as_str())
            .collect();
        assert_eq!(order, ["Zoe", "amy", "Bea", "Abe", "zed"]);
    }

    #[test]
    fn a_cloud_is_reached_over_https_or_on_this_pc() {
        assert!(is_cloud_address("https://cloud.example.org"));
        assert!(is_cloud_address(" https://cloud.example.org:8443/ "));
        assert!(is_cloud_address("http://127.0.0.1:8080"));
        assert!(is_cloud_address("http://localhost:8080"));
        assert!(is_cloud_address("http://[::1]:8080"));
        assert!(!is_cloud_address("http://cloud.example.org"));
        assert!(!is_cloud_address("http://192.168.1.4:8080"));
        assert!(!is_cloud_address("https://"));
        assert!(!is_cloud_address("cloud.example.org"));
        assert!(!is_cloud_address("https://a b.org"));
        assert_eq!(
            cloud(Some("https://mine.example.org")),
            Some("https://mine.example.org")
        );
        assert_eq!(cloud(None), DEFAULT_CLOUD.filter(|u| !u.is_empty()));
    }

    #[test]
    fn a_game_gets_only_the_bridge_variables() {
        let mut bridge = BTreeMap::new();
        bridge.insert(
            "ANYPS5_NET_ENDPOINT".to_owned(),
            "127.0.0.1:5000".to_owned(),
        );
        bridge.insert("ANYPS5_NET_TOKEN".to_owned(), "ab".to_owned());
        bridge.insert("PATH".to_owned(), "nope".to_owned());
        let env = game_environment(Some(&bridge), Some(Path::new("C:/Logs/X-network.jsonl")));
        assert_eq!(env.len(), 3);
        assert!(!env.contains_key("PATH"));
        assert_eq!(env["ANYPS5_NET_OBSERVE"], "C:/Logs/X-network.jsonl");
        assert!(game_environment(None, None).is_empty());
    }
}
