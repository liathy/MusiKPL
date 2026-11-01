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
/// Last.fm users removed here, so following them doesn't bring them back.
const HIDDEN_FILE: &str = "friends-hidden.json";
/// How often who the user follows on Last.fm is read again.
const FOLLOW_SYNC: Duration = Duration::from_secs(15 * 60);
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
    /// Added because the user follows them on Last.fm (so unfollowing
    /// them there removes them here too).
    #[serde(default)]
    pub from_lastfm: bool,
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
    /// Last.fm users removed here (lower-case), not re-added by the sync.
    pub hidden: Vec<String>,
    /// When who the user follows on Last.fm was last asked for.
    follows_checked: Option<Instant>,
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
        self.friends.hidden = storage::read(HIDDEN_FILE).unwrap_or_default();
    }

    /// Every few minutes, with Last.fm connected: read who the user follows
    /// there, so friends are the same on every device.
    fn sync_follows(&mut self) {
        if self
            .friends
            .follows_checked
            .is_some_and(|at| at.elapsed() < FOLLOW_SYNC)
        {
            return;
        }
        let user = self.settings.lastfm.username.clone();
        let Some(api_key) = crate::lastfm::lookup_key(&self.settings.lastfm.api_key) else {
            return;
        };
        if user.is_empty() || !self.lastfm_connected() {
            return;
        }
        self.friends.follows_checked = Some(Instant::now());
        self.lastfm
            .browse(crate::lastfm::BrowseRequest::Following { api_key, user });
    }

    /// Check who the user follows on Last.fm on the next tick.
    pub fn refresh_follows(&mut self) {
        self.friends.follows_checked = None;
    }

    /// Who the user follows on Last.fm arrived: they're friends here too
    /// (unless removed here). Friends who came from Last.fm and were
    /// unfollowed there go, unless a friend code was added for them.
    pub fn receive_follows(&mut self, followed: Vec<crate::lastfm::browse::Followed>) {
        let mut changed = false;
        let names: std::collections::HashSet<String> = followed
            .iter()
            .map(|person| person.name.to_lowercase())
            .collect();
        for person in followed {
            let key = person.name.to_lowercase();
            if self.friends.hidden.contains(&key) {
                continue;
            }
            let existing = self.friends.list.iter_mut().find(|friend| {
                friend
                    .lastfm
                    .as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case(&person.name))
            });
            match existing {
                Some(friend) => {
                    if friend.avatar.is_none() && person.image.is_some() {
                        friend.avatar = person.image;
                        changed = true;
                    }
                }
                None => {
                    self.friends.list.push(Friend {
                        name: person.name.clone(),
                        lastfm: Some(person.name),
                        id: None,
                        avatar: person.image,
                        from_lastfm: true,
                    });
                    changed = true;
                }
            }
        }
        let before = self.friends.list.len();
        self.friends.list.retain(|friend| {
            let unfollowed = friend
                .lastfm
                .as_deref()
                .is_some_and(|name| !names.contains(&name.to_lowercase()));
            !(friend.from_lastfm && friend.id.is_none() && unfollowed)
        });
        if changed || self.friends.list.len() != before {
            self.save_friends();
        }
    }

    /// Remember a Last.fm user was removed here, so the sync leaves them out.
    fn hide_follow(&mut self, name: &str) {
        let key = name.to_lowercase();
        if !self.friends.hidden.contains(&key) {
            self.friends.hidden.push(key);
            storage::write_in_background(HIDDEN_FILE, &self.friends.hidden);
        }
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
        self.allow_peers();
        // A friend who was removed shouldn't keep a pending invite.
        self.friends
            .invites
            .retain(|invite| ids.contains(&invite.id));
    }

    /// Who may reach this device: friends and the user's linked devices.
    fn peers(&self) -> Vec<String> {
        let mut ids = self.friends.ids();
        ids.extend(self.devices.ids());
        ids
    }

    pub(super) fn allow_peers(&self) {
        if let Some(service) = &self.friends.service {
            service.allow(&self.peers());
        }
    }

    pub(super) fn friends_service(&self) -> Option<&Friends> {
        self.friends.service.as_ref()
    }

    /// Start the friends network if it isn't running.
    pub(super) fn ensure_friends_service(&mut self) {
        if self.friends.service.is_none() {
            self.friends.service = Some(Friends::start(
                &self.peers(),
                &self.friend_name(),
                self.waker.clone(),
            ));
        }
    }

    /// The name friends see: the Last.fm username, else the PC's user name.
    pub fn friend_name(&self) -> String {
        let lastfm = &self.settings.lastfm.username;
        if !lastfm.is_empty() {
            return lastfm.clone();
        }
        std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .ok()
            .or_else(crate::platform::device_name)
            .unwrap_or_else(|| "MusiKPL user".into())
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
        let key = username.to_lowercase();
        if self.friends.hidden.contains(&key) {
            self.friends.hidden.retain(|name| *name != key);
            storage::write_in_background(HIDDEN_FILE, &self.friends.hidden);
        }
        self.friends.list.push(Friend {
            name: username.to_owned(),
            lastfm: Some(username.to_owned()),
            id: None,
            avatar: None,
            from_lastfm: true,
        });
        self.save_friends();
        // Following them on Last.fm makes them a friend on every device
        // (Last.fm lets apps read follows, not make them).
        let _ = crate::platform::open_url(&format!("https://www.last.fm/user/{username}"));
        self.set_friend_notice(format!(
            "Added {username}. Press Follow on their Last.fm page to have them on all your devices, and swap friend codes to see each other online."
        ));
    }

    pub fn remove_lastfm_friend(&mut self, username: &str) {
        self.hide_follow(username);
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
            let friend = self.friends.list.remove(index);
            if let Some(name) = &friend.lastfm {
                self.hide_follow(name);
            }
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
                from_lastfm: false,
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
        self.sync_follows();
        let ids = self.friends.ids();
        // Also running with Last.fm connected, so the user's other devices
        // can ask to link.
        let needed = !ids.is_empty()
            || !self.devices.linked.is_empty()
            || (self.lastfm_connected() && !self.settings.lastfm.username.is_empty());
        if !needed {
            return;
        }
        self.ensure_friends_service();
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
                FriendEvent::LinkRequested { id, name, lastfm } => {
                    self.link_requested(&id, &name, &lastfm);
                }
                FriendEvent::LinkSent { id, delivered } => {
                    if !delivered {
                        self.friends.online.insert(id, false);
                    }
                }
                FriendEvent::Synced { id, data } => self.receive_sync(&id, &data),
                FriendEvent::SyncSent { id, delivered } => self.sync_finished(&id, delivered),
                FriendEvent::Failed(error) => self.set_friend_notice(error),
            }
        }
        if ids.is_empty() {
            return;
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
    fn people_followed_on_lastfm_are_friends_everywhere() {
        use crate::lastfm::browse::Followed;
        let follow = |name: &str| Followed {
            name: name.into(),
            image: None,
        };
        let mut app = Engine::default();
        app.receive_follows(vec![follow("ana"), follow("bia")]);
        assert_eq!(app.friends.list.len(), 2);
        // A friend code added for Bia keeps her when unfollowed.
        app.friends.list[1].id = Some("key".into());
        app.receive_follows(vec![]);
        assert_eq!(app.friends.list.len(), 1);
        assert_eq!(app.friends.list[0].name, "bia");
        // Removed here: following doesn't bring them back.
        app.receive_follows(vec![follow("cid")]);
        let cid = app
            .friends
            .list
            .iter()
            .position(|f| f.name == "cid")
            .unwrap();
        app.remove_friend(cid);
        app.receive_follows(vec![follow("cid")]);
        assert!(!app.friends.is_friend("cid"));
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
