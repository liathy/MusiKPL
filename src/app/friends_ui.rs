//! The Friends card in the menu: what it shows, and its buttons.

use musikpl_core::{engine::Friend, jam::Role};

use crate::ui::menu::{FriendAction, FriendItem, FriendStatus, FriendsView};

use super::YtMusicApp;

impl YtMusicApp {
    /// What the Friends card shows.
    pub fn friends_view(&self) -> FriendsView<'_> {
        let items =
            self.friends
                .list
                .iter()
                .map(|friend| {
                    let status = match &friend.id {
                        None => FriendStatus::NoCode,
                        Some(id) => match self.friends.online.get(id) {
                            Some(true) => FriendStatus::Online,
                            Some(false) => FriendStatus::Offline,
                            None => FriendStatus::Unknown,
                        },
                    };
                    let invited_us = friend.id.as_ref().is_some_and(|id| {
                        self.friends.invites.iter().any(|invite| &invite.id == id)
                    });
                    FriendItem {
                        name: &friend.name,
                        avatar: friend.avatar.clone().map(|url| crate::domain::Cover {
                            url,
                            uv: [0.0, 0.0, 1.0, 1.0],
                        }),
                        status,
                        invited_us,
                    }
                })
                .collect();
        FriendsView {
            items,
            hosting: self.jam.info.role == Some(Role::Host),
            quiet_invites: !self.settings.invite_popups,
            adding: self.friends.adding,
            notice: self.friends.notice(),
        }
    }

    pub fn apply_friend_action(&mut self, action: FriendAction) {
        let friend = |index: usize| self.friends.list.get(index).cloned();
        match action {
            FriendAction::Open(index) => match friend(index) {
                Some(Friend {
                    lastfm: Some(user), ..
                }) => self.open_user_profile(&user),
                Some(friend) => self
                    .set_friend_notice(format!("{} has no Last.fm profile linked.", friend.name)),
                None => {}
            },
            FriendAction::Remove(index) => self.remove_friend(index),
            FriendAction::Invite(index) => {
                if let Some(id) = friend(index).and_then(|friend| friend.id) {
                    self.invite_friend(&id);
                }
            }
            FriendAction::Join(index) => {
                if let Some(id) = friend(index).and_then(|friend| friend.id) {
                    self.join_invite_from(&id);
                }
            }
            FriendAction::CopyCode => {
                self.clipboard = Some(self.my_friend_code());
                self.set_friend_notice("Your friend code is copied. Send it to a friend.");
            }
            FriendAction::StartAdding => self.friends.adding = true,
            FriendAction::CancelAdding => {
                self.friends.adding = false;
                self.friends.code_input.clear();
            }
            FriendAction::AddCode => self.add_friend_code(),
        }
    }

    /// Join the Jam a friend invited us to, and show it in the menu.
    pub(super) fn join_invite_from(&mut self, id: &str) {
        if self.engine.accept_invite(id) {
            self.menu_open = true;
            self.menu_jam_open = true;
        }
    }
}
