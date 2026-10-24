//! The online panel in the panel host: what the player asks for becomes a command for the net
//! service (`Model::online`), and a join first checks the library and the versions.

use anyps5_core::online::api::AccountId;
use anyps5_core::online::{self as net, Command, Join, Purpose, Status};

use super::PanelHost;
use crate::app::model::{Effect, Model};
use crate::app::online::OnlineService;
use crate::app::overlay::{Overlay, OverlayCx};
use crate::screens::online::{OnlineAction, OnlineData};
use crate::screens::online_strings::OnlineText as T;
use crate::ui::icons::Icon;

impl PanelHost {
    pub(super) fn online_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        let events = self.take_events(wanted);
        let side_next = cx.model.overlay.is_some_and(Overlay::is_side_panel);
        let (language, accent) = (cx.model.language(), cx.shell.accent_color());
        let cloud = net::cloud(cx.model.prefs.online_cloud.as_deref()).map(str::to_owned);
        // Opening the panel brings the net service up: it is local until the player signs in.
        if wanted
            && !self.online_panel.is_active()
            && cloud.is_some()
            && !matches!(cx.model.online.status, Status::Running | Status::Missing)
        {
            cx.model.effects.push(Effect::StartOnline);
        }
        let actions = {
            let data = OnlineData {
                online: &cx.model.online,
                cloud: cloud.as_deref(),
                custom_cloud: cx.model.prefs.online_cloud.is_some(),
                official_cloud: net::DEFAULT_CLOUD.is_some(),
                language,
                accent,
            };
            if wanted && !self.online_panel.is_open() {
                self.online_panel.open(&data);
            } else if !wanted && self.online_panel.is_active() && self.online_panel.is_open() {
                if side_next {
                    self.online_panel.dismiss();
                } else {
                    self.online_panel.close();
                }
            }
            self.online_panel.show(cx.shell, cx.ctx, &events, &data)
        };
        if wanted && cx.model.online.status == Status::Missing {
            let text = T::ServiceMissing.text(language);
            if cx.model.toast.as_ref().is_none_or(|t| t.text != text) {
                cx.model.toast(text, Icon::WARNING);
            }
        }
        for action in actions {
            online_action(cx.model, action);
        }
    }
}

fn online_action(model: &mut Model, action: OnlineAction) {
    let online = &mut model.online;
    match action {
        OnlineAction::SetCloud(url) => {
            let url = url.trim().trim_end_matches('/').to_owned();
            model.edit_prefs(|p| p.online_cloud = Some(url));
            model.effects.push(Effect::StartOnline);
        }
        OnlineAction::UseOfficialCloud => {
            model.edit_prefs(|p| p.online_cloud = None);
            model.effects.push(Effect::StartOnline);
        }
        OnlineAction::Email(email) => {
            online.send(Purpose::Email(email.clone()), Command::Email { email });
        }
        OnlineAction::Code(code) => {
            online.send(Purpose::Code, Command::Code { code });
        }
        OnlineAction::SignUp(handle) => {
            online.send(
                Purpose::SignUp,
                Command::SignUp {
                    handle,
                    age_confirmed: true,
                },
            );
        }
        OnlineAction::OtherAddress => {}
        OnlineAction::AddFriend(handle) => {
            online.send(
                Purpose::AddFriend(handle.clone()),
                Command::AddFriend { handle },
            );
        }
        OnlineAction::Accept(account) => {
            online.send(Purpose::Accept(account), Command::Accept { account });
        }
        OnlineAction::Decline(account) => {
            online.send(Purpose::Decline(account), Command::Decline { account });
        }
        OnlineAction::RemoveFriend(account) => {
            online.send(
                Purpose::RemoveFriend(account),
                Command::RemoveFriend { account },
            );
        }
        OnlineAction::Block(account) => {
            online.send(Purpose::Block(account), Command::Block { account });
        }
        OnlineAction::Unblock(account) => {
            online.send(Purpose::Unblock(account), Command::Unblock { account });
        }
        OnlineAction::Report(account, reason) => {
            online.send(
                Purpose::Report(account),
                Command::Report {
                    account,
                    reason,
                    details: String::new(),
                },
            );
        }
        OnlineAction::Join(friend) => {
            if let Some(join) = friend_join(model, friend) {
                OnlineService::request_join(model, join);
            }
        }
        OnlineAction::Invite(account) => {
            online.send(
                Purpose::Invite,
                Command::Invite {
                    accounts: vec![account],
                },
            );
        }
        OnlineAction::AnswerInvitation { id, accept: true } => {
            // Accepting is joining: the net service finds the invitation by its session.
            let invitation = online.invitations.iter().find(|i| i.id == id).cloned();
            if let Some(invitation) = invitation {
                OnlineService::request_join(
                    model,
                    Join {
                        session: invitation.session,
                        title_id: invitation.title_id,
                        title: invitation.title,
                        version: invitation.version,
                        build: String::new(),
                        friend: invitation.from.handle,
                    },
                );
            }
        }
        OnlineAction::AnswerInvitation { id, accept: false } => {
            online.send(
                Purpose::AnswerInvitation {
                    id: id.clone(),
                    accept: false,
                },
                Command::AnswerInvitation { id, accept: false },
            );
        }
        OnlineAction::SignOut => {
            online.send(Purpose::SignOut, Command::SignOut);
        }
        OnlineAction::DeleteAccount => {
            online.send(Purpose::DeleteAccount, Command::DeleteAccount);
        }
        OnlineAction::Dialog(accounts) => {
            if let Some(dialog) = online.dialog.take() {
                online.send(
                    Purpose::Dialog,
                    Command::Dialog {
                        id: dialog.id,
                        accounts,
                    },
                );
            }
        }
        OnlineAction::Close => model.dismiss_overlay(),
    }
}

/// The session a friend plays in, when friends may join it.
fn friend_join(model: &Model, friend: AccountId) -> Option<Join> {
    let friend = model.online.friend(friend)?;
    let activity = friend.presence.activity.as_ref()?;
    let session = activity.session.as_ref().filter(|s| s.joinable)?;
    Some(Join {
        session: session.id.clone(),
        title_id: activity.title_id.clone(),
        title: activity.title.clone(),
        version: activity.version.clone(),
        build: String::new(),
        friend: friend.account.handle.clone(),
    })
}
