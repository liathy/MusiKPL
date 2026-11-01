//! Friends in the app: the list (saved in friends.json), whether each is
//! online in MusiKPL, and Jam invites both ways.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use crate::{
    friends::{my_code, FriendCode, FriendEvent, Friends},
    jam::{Progress, Role},
    storage,
};

use super::Engine;

const FRIENDS_FILE: &str = "friends.json";
/// How often presence is checked: often while the menu (and its Friends
/// card) is open, rarely otherwise, to keep the network quiet.
const CHECK_OPEN: Duration = Duration::from_secs(60);
const CHECK_CLOSED: Duration = Duration::from_secs(5 * 60);
/// How long a Friends message stays in the card.
const NOTICE_TIME: Duration = Duration::from_secs(8);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Friend {
    pub name: String,
    /// Their Last.fm username, for their profile.
    #[serde(default)]
    pub lastfm: Option<String>,
    /// Their friend code's key; without it they can't be seen online.
    #[serde(default)]
    pub id: Option<String>,
    /// Their Last.fm picture, remembered between runs.
    #[serde(default)]
    pub avatar: Option<String>,
}

/// A Jam invite from a friend, waiting for Join or Not now.
#[derive(Clone, Debug, PartialEq)]
pub struct ReceivedInvite {
    pub id: String,
    pub name: String,
    pub invite: String,
}

#[derive(Default)]
pub struct FriendsState {
    pub list: Vec<Friend>,
    service: Option<Friends>,
    pub online: HashMap<String, bool>,
    last_check: Option<Instant>,
    pub invites: Vec<ReceivedInvite>,
    /// Friends to invite once this Jam's internet invite is ready.
    pending: Vec<String>,
    /// The friend code being pasted in the Friends card.
    pub code_input: String,
    pub adding: bool,
    notice: Option<(String, Instant)>,
    /// Last.fm users whose picture was asked for this run.
    pub avatars_asked: std::collections::HashSet<String>,
}

impl FriendsState {
    pub fn notice(&self) -> Option<&str> {
        self.notice
            .as_ref()
            .filter(|(_, at)| at.elapsed() < NOTICE_TIME)
            .map(|(text, _)| text.as_str())
    }

    fn ids(&self) -> Vec<String> {
        self.list
            .iter()
            .filter_map(|friend| friend.id.clone())
            .collect()
    }

    pub fn is_friend(&self, lastfm: &str) -> bool {
        self.list.iter().any(|friend| {
            friend
                .lastfm
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case(lastfm))
        })
    }
}

impl Engine {
    pub fn load_friends(&mut self) {
        self.friends.list = storage::read(FRIENDS_FILE).unwrap_or_default();
    }

    /// A friend's Last.fm picture arrived: keep it if it changed.
    pub fn set_friend_avatar(&mut self, user: &str, image: Option<String>) {
        let mut changed = false;
        for friend in &mut self.friends.list {
            let theirs = friend
                .lastfm
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case(user));
            if theirs && friend.avatar != image {
                friend.avatar.clone_from(&image);
                changed = true;
            }
        }
        if changed {
            storage::write_in_background(FRIENDS_FILE, &self.friends.list);
        }
    }

    fn save_friends(&mut self) {
        storage::write_in_background(FRIENDS_FILE, &self.friends.list);
        let ids = self.friends.ids();
        if let Some(service) = &self.friends.service {
            service.allow(&ids);
        }
        // A friend who was removed shouldn't keep a pending invite.
        self.friends
            .invites
            .retain(|invite| ids.contains(&invite.id));
    }

    /// The name friends see: the Last.fm username, else the PC's user name.
    pub fn friend_name(&self) -> String {
        let lastfm = &self.settings.lastfm.username;
        if !lastfm.is_empty() {
            return lastfm.clone();
        }
        std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_else(|_| "MusiKPL user".into())
    }

    pub fn my_friend_code(&self) -> String {
        my_code(&self.friend_name()).encode()
    }

    pub fn set_friend_notice(&mut self, text: impl Into<String>) {
        self.friends.notice = Some((text.into(), Instant::now()));
    }

    /// Add a Last.fm user as a friend (from their profile).
    pub fn add_lastfm_friend(&mut self, username: &str) {
        if self.friends.is_friend(username) {
            return;
        }
        self.friends.list.push(Friend {
            name: username.to_owned(),
            lastfm: Some(username.to_owned()),
            id: None,
            avatar: None,
        });
        self.save_friends();
        self.set_friend_notice(format!(
            "Added {username}. Swap friend codes to see each other online."
        ));
    }

    pub fn remove_lastfm_friend(&mut self, username: &str) {
        self.friends.list.retain(|friend| {
            !friend
                .lastfm
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case(username))
        });
        self.save_friends();
    }

    pub fn remove_friend(&mut self, index: usize) {
        if index < self.friends.list.len() {
            self.friends.list.remove(index);
            self.save_friends();
        }
    }

    /// Add the friend code pasted in the Friends card. It completes a
    /// friend added from Last.fm with the same name, or adds a new one.
    pub fn add_friend_code(&mut self) {
        let code = match FriendCode::decode(&self.friends.code_input) {
            Ok(code) => code,
            Err(error) => {
                self.set_friend_notice(error);
                return;
            }
        };
        if code.id == my_code("").id {
            self.set_friend_notice("That's your own friend code.");
            return;
        }
        if self
            .friends
            .list
            .iter()
            .any(|f| f.id.as_deref() == Some(&code.id))
        {
            self.set_friend_notice(format!("{} is already your friend.", code.name));
            self.friends.code_input.clear();
            return;
        }
        let existing = self.friends.list.iter_mut().find(|friend| {
            friend.id.is_none()
                && (friend.name.eq_ignore_ascii_case(&code.name)
                    || friend
                        .lastfm
                        .as_deref()
                        .is_some_and(|name| name.eq_ignore_ascii_case(&code.name)))
        });
        match existing {
            Some(friend) => friend.id = Some(code.id.clone()),
            None => self.friends.list.push(Friend {
                name: code.name.clone(),
                lastfm: None,
                id: Some(code.id.clone()),
                avatar: None,
            }),
        }
        self.friends.code_input.clear();
        self.friends.adding = false;
        self.save_friends();
        self.set_friend_notice(format!(
            "Added {}. They see you online once they add your code too.",
            code.name
        ));
        // Check right away.
        self.friends.last_check = None;
    }

    /// Called every frame: run the service when there are friends to reach,
    /// check presence now and then, and handle invites.
    pub fn tick_friends(&mut self) {
        self.request_friend_avatars();
        let ids = self.friends.ids();
        if ids.is_empty() {
            return;
        }
        if self.friends.service.is_none() {
            self.friends.service = Some(Friends::start(
                &ids,
                &self.friend_name(),
                self.waker.clone(),
            ));
        }
        let events = self
            .friends
            .service
            .as_ref()
            .map(Friends::poll)
            .unwrap_or_default();
        for event in events {
            match event {
                FriendEvent::Presence { id, online } => {
                    self.friends.online.insert(id, online);
                }
                FriendEvent::Invited { id, name, invite } => {
                    // A friend who invites is online.
                    self.friends.online.insert(id.clone(), true);
                    self.friends.invites.retain(|old| old.id != id);
                    let name = self
                        .friends
                        .list
                        .iter()
                        .find(|friend| friend.id.as_deref() == Some(&id))
                        .map_or(name, |friend| friend.name.clone());
                    self.friends
                        .invites
                        .push(ReceivedInvite { id, name, invite });
                }
                FriendEvent::InviteSent { id, delivered } => {
                    let name = self.friend_display(&id);
                    self.set_friend_notice(if delivered {
                        format!("Invited {name} to your Jam.")
                    } else {
                        format!("Couldn't reach {name}. Are they online?")
                    });
                    if !delivered {
                        self.friends.online.insert(id, false);
                    }
                }
                FriendEvent::Failed(error) => self.set_friend_notice(error),
            }
        }

        let interval = if self.menu_visible {
            CHECK_OPEN
        } else {
            CHECK_CLOSED
        };
        if self
            .friends
            .last_check
            .is_none_or(|at| at.elapsed() >= interval)
        {
            self.friends.last_check = Some(Instant::now());
            if let Some(service) = &self.friends.service {
                service.set_name(&self.friend_name());
                service.check(ids);
            }
        }

        self.send_pending_invites();
    }

    /// Check presence right away (the menu was opened).
    pub fn refresh_friends(&mut self) {
        let stale = self
            .friends
            .last_check
            .is_none_or(|at| at.elapsed() >= Duration::from_secs(15));
        if stale {
            self.friends.last_check = None;
        }
    }

    fn friend_display(&self, id: &str) -> String {
        self.friends
            .list
            .iter()
            .find(|friend| friend.id.as_deref() == Some(id))
            .map_or_else(|| "your friend".into(), |friend| friend.name.clone())
    }

    /// Invite a friend to the Jam this device hosts; the Jam is shared over
    /// the internet first if it isn't yet.
    pub fn invite_friend(&mut self, id: &str) {
        if self.jam.info.role != Some(Role::Host) {
            self.set_friend_notice("Start a Jam first (Menu → Jam), then invite friends.");
            return;
        }
        match self.jam.internet_status() {
            Some(Progress::Failed(error)) => {
                self.set_friend_notice(error);
                return;
            }
            None => self.jam.share_over_internet(),
            _ => {}
        }
        if !self.friends.pending.iter().any(|pending| pending == id) {
            self.friends.pending.push(id.to_owned());
        }
        self.set_friend_notice(format!("Inviting {}…", self.friend_display(id)));
        self.send_pending_invites();
    }

    fn send_pending_invites(&mut self) {
        if self.friends.pending.is_empty() {
            return;
        }
        if self.jam.info.role != Some(Role::Host) {
            self.friends.pending.clear();
            return;
        }
        let Some(Progress::Ready(invite)) = self.jam.internet_status() else {
            return;
        };
        let Some(service) = &self.friends.service else {
            return;
        };
        for id in self.friends.pending.drain(..) {
            service.invite(id, invite.clone());
        }
    }

    /// Join the Jam a friend invited us to. True when joining started (the
    /// shell then shows the Jam).
    pub fn accept_invite(&mut self, id: &str) -> bool {
        let Some(index) = self
            .friends
            .invites
            .iter()
            .position(|invite| invite.id == id)
        else {
            return false;
        };
        let invite = self.friends.invites.remove(index);
        if self.jam.info.role.is_some() {
            self.jam.leave();
        }
        match self.jam.join_invite(&invite.invite) {
            Ok(()) => {
                self.set_friend_notice(format!("Joining {}'s Jam…", invite.name));
                true
            }
            Err(error) => {
                self.set_friend_notice(error);
                false
            }
        }
    }

    pub fn decline_invite(&mut self, id: &str) {
        self.friends.invites.retain(|invite| invite.id != id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;

    fn code(name: &str) -> String {
        FriendCode {
            id: SecretKey::generate().public().to_string(),
            name: name.into(),
        }
        .encode()
    }

    #[test]
    fn a_friend_code_completes_a_lastfm_friend_or_adds_one() {
        let mut app = Engine::default();
        app.add_lastfm_friend("Ana");
        assert!(app.friends.is_friend("ana"));
        assert!(app.friends.list[0].id.is_none());

        app.friends.code_input = code("ana");
        app.add_friend_code();
        assert_eq!(app.friends.list.len(), 1);
        assert!(app.friends.list[0].id.is_some());

        app.friends.code_input = code("Bia");
        app.add_friend_code();
        assert_eq!(app.friends.list.len(), 2);

        app.friends.code_input = "nonsense".into();
        app.add_friend_code();
        assert_eq!(app.friends.list.len(), 2);
        assert!(app.friends.notice().is_some());
    }

    #[test]
    fn inviting_needs_a_jam_and_invites_can_be_declined() {
        let mut app = Engine::default();
        app.invite_friend("someone");
        assert!(app.friends.notice().unwrap().contains("Start a Jam"));

        app.friends.invites.push(ReceivedInvite {
            id: "x".into(),
            name: "Ana".into(),
            invite: "musikpl:...".into(),
        });
        app.decline_invite("x");
        assert!(app.friends.invites.is_empty());
    }
}
