package io.github.liathy.musikpl.ui

import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.LaunchedEffect
import androidx.core.content.ContextCompat
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.liathy.musikpl.R
import io.github.liathy.musikpl.core.Core
import uniffi.musikpl_mobile.FriendItem
import uniffi.musikpl_mobile.FriendStatus
import uniffi.musikpl_mobile.InternetShare
import uniffi.musikpl_mobile.JamRole
import uniffi.musikpl_mobile.JamState

/** Jams (listening together) and friends. */
@Composable
fun JamScreen(nav: Nav) {
    val jam by Core.jam.collectAsStateWithLifecycle()
    val friends by Core.friends.collectAsStateWithLifecycle()
    var addingCode by remember { mutableStateOf(false) }
    var joiningInvite by remember { mutableStateOf(false) }
    var removing by remember { mutableStateOf<FriendItem?>(null) }

    // Android 17 asks before apps reach other devices on the Wi-Fi.
    val context = LocalContext.current
    val askLocalNetwork = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {}
    LaunchedEffect(Unit) {
        val permission = "android.permission.ACCESS_LOCAL_NETWORK"
        val granted = ContextCompat.checkSelfPermission(context, permission) == PackageManager.PERMISSION_GRANTED
        if (Build.VERSION.SDK_INT >= 36 && !granted) askLocalNetwork.launch(permission)
    }

    // While this shows: look for nearby Jams and check friends often.
    DisposableEffect(Unit) {
        Core.jamVisible = true
        Core.act { setFriendsVisible(true) }
        onDispose {
            Core.jamVisible = false
            Core.act { setFriendsVisible(false) }
        }
    }

    LazyColumn(contentPadding = PaddingValues(bottom = 16.dp)) {
        item {
            Text("Jam", style = MaterialTheme.typography.headlineMedium, modifier = Modifier.padding(start = 16.dp, top = 12.dp))
            Text(
                "Listen together in sync. Everyone streams the music themselves; the Jam keeps playback together, end-to-end encrypted.",
                style = MaterialTheme.typography.bodyMedium,
                color = Palette.secondary,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
            )
        }
        jam?.let { state -> item { JamCard(state, onJoinInvite = { joiningInvite = true }) } }
        val nearby = jam?.nearby.orEmpty()
        if (jam?.role == JamRole.NONE) {
            item { SectionTitle("Nearby Jams") }
            if (nearby.isEmpty()) {
                item {
                    Text(
                        jam?.discoveryProblem ?: "Jams on this Wi-Fi show up here.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = Palette.secondary,
                        modifier = Modifier.padding(horizontal = 16.dp),
                    )
                }
            }
            items(nearby, key = { it.sessionId.toString() }) { nearbyJam ->
                Row(
                    Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(Modifier.weight(1f)) {
                        ScrollingText("${nearbyJam.host}'s Jam", MaterialTheme.typography.titleMedium)
                        ScrollingText(
                            listOfNotNull(nearbyJam.track, "${nearbyJam.listeners} listening").joinToString(" · "),
                            MaterialTheme.typography.bodySmall,
                            color = Palette.secondary,
                        )
                    }
                    Pill("Join", { Core.act { joinNearbyJam(nearbyJam.sessionId) } })
                }
            }
        }

        item {
            SectionTitle("Friends") {
                IconAction(R.drawable.ic_add, "Add a friend code", { addingCode = true })
            }
        }
        item {
            Row(Modifier.padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Pill("Copy my code", { Core.act { copyFriendCode() } }, primary = false)
                Pill("Add a code", { addingCode = true }, primary = false)
            }
        }
        friends?.notice?.let { notice ->
            item { Text(notice, color = Palette.secondary, modifier = Modifier.padding(16.dp)) }
        }
        val state = friends
        if (state != null) {
            // Invites that wait here (pop-ups off).
            if (!state.invitePopups) {
                items(state.invites, key = { "invite:" + it.id }) { invite ->
                    Row(
                        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text("${invite.name} invited you to their Jam", modifier = Modifier.weight(1f))
                        Pill("Join", { Core.act { acceptFriendInvite(invite.id) } })
                    }
                }
            }
            if (state.friends.isEmpty()) {
                item {
                    Text(
                        "People you follow on Last.fm show up here on all your devices. Swap friend codes to see each other online and invite each other to Jams.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = Palette.secondary,
                        modifier = Modifier.padding(16.dp),
                    )
                }
            }
            items(state.friends, key = { "friend:" + it.index + it.name }) { friend ->
                FriendRow(friend, state.hosting, nav, onRemove = { removing = friend })
            }
        }
    }

    if (addingCode) {
        CodeDialog("Add a friend code", "Paste your friend's code", onDone = { code ->
            addingCode = false
            Core.act { addFriendCode(code) }
        }, onCancel = { addingCode = false })
    }
    if (joiningInvite) {
        CodeDialog("Join with an invite", "Paste the Jam invite", onDone = { invite ->
            joiningInvite = false
            Core.act { joinJamInvite(invite) }
        }, onCancel = { joiningInvite = false })
    }
    removing?.let { friend ->
        AlertDialog(
            onDismissRequest = { removing = null },
            containerColor = Palette.panel,
            title = { Text("Remove ${friend.name}?", style = MaterialTheme.typography.titleLarge) },
            text = { Text("They leave your friends on this device. Unfollow them on Last.fm to remove them everywhere.") },
            confirmButton = {
                TextButton(onClick = {
                    removing = null
                    Core.act { removeFriend(friend.index) }
                }) { Text("Remove", color = Palette.danger) }
            },
            dismissButton = { TextButton(onClick = { removing = null }) { Text("Cancel") } },
        )
    }
}

@Composable
private fun JamCard(state: JamState, onJoinInvite: () -> Unit) {
    val context = LocalContext.current
    Column(
        Modifier
            .fillMaxWidth()
            .padding(16.dp)
            .clip(RoundedCornerShape(12.dp))
            .background(Palette.panel)
            .padding(16.dp),
    ) {
        when (state.role) {
            JamRole.NONE -> {
                Text("Start a Jam and others join from their phone or PC.", color = Palette.secondary)
                Spacer(Modifier.size(12.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Pill("Start a Jam", { Core.act { startJam() } })
                    Pill("Join with an invite", onJoinInvite, primary = false)
                }
            }
            JamRole.HOST -> {
                Text("You're hosting", style = MaterialTheme.typography.titleLarge)
                Text(
                    "${state.listeners} listening · ${state.status}",
                    style = MaterialTheme.typography.bodyMedium,
                    color = Palette.secondary,
                )
                if (state.waitingForGuests) {
                    Text("Waiting for everyone to load the song…", color = Palette.secondary)
                }
                Spacer(Modifier.size(12.dp))
                when (val internet = state.internet) {
                    InternetShare.Off -> Pill("Share over the internet", { Core.act { shareJamOverInternet() } }, primary = false)
                    InternetShare.Preparing -> Text("Preparing the internet invite…", color = Palette.secondary)
                    is InternetShare.Ready -> {
                        Text("Send this invite to friends anywhere:", color = Palette.secondary)
                        Spacer(Modifier.size(8.dp))
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            Pill("Share invite", {
                                val send = Intent(Intent.ACTION_SEND).setType("text/plain")
                                    .putExtra(Intent.EXTRA_TEXT, internet.invite)
                                context.startActivity(Intent.createChooser(send, "Jam invite"))
                            })
                            Pill("Copy", { Core.clipboard.value = internet.invite }, primary = false)
                        }
                        Spacer(Modifier.size(8.dp))
                        androidx.compose.foundation.text.selection.SelectionContainer {
                            Text(
                                internet.invite,
                                style = MaterialTheme.typography.bodySmall,
                                color = Palette.muted,
                                maxLines = 3,
                                overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis,
                            )
                        }
                    }
                    is InternetShare.Failed -> {
                        Text(internet.reason, color = Palette.danger)
                        Pill("Try again", { Core.act { shareJamOverInternet() } }, primary = false)
                    }
                }
                Spacer(Modifier.size(12.dp))
                Pill("End the Jam", { Core.act { endJam() } }, primary = false)
            }
            JamRole.GUEST -> {
                Text(
                    if (state.connecting) "Connecting to ${state.hostName}…" else "In ${state.hostName}'s Jam",
                    style = MaterialTheme.typography.titleLarge,
                )
                Text(state.status, style = MaterialTheme.typography.bodyMedium, color = Palette.secondary)
                Text(
                    "Songs you add go to the host's queue.",
                    style = MaterialTheme.typography.bodySmall,
                    color = Palette.muted,
                )
                Spacer(Modifier.size(12.dp))
                Pill("Leave", { Core.act { endJam() } }, primary = false)
            }
        }
        state.notice?.let {
            Spacer(Modifier.size(8.dp))
            Text(it, style = MaterialTheme.typography.bodySmall, color = Palette.secondary)
        }
    }
}

@Composable
private fun FriendRow(friend: FriendItem, hosting: Boolean, nav: Nav, onRemove: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(enabled = friend.lastfm != null) { friend.lastfm?.let { nav.profile(it) } }
            .padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box {
            RoundPhoto(friend.avatar, friend.name, Modifier.size(48.dp))
            if (friend.status == FriendStatus.ONLINE) {
                Box(
                    Modifier.align(Alignment.BottomEnd).size(14.dp).clip(CircleShape)
                        .background(Palette.background).padding(2.dp).clip(CircleShape).background(Palette.success),
                )
            }
        }
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            ScrollingText(friend.name, MaterialTheme.typography.titleMedium)
            Text(
                when (friend.status) {
                    FriendStatus.ONLINE -> "Online"
                    FriendStatus.OFFLINE -> "Offline"
                    FriendStatus.UNKNOWN -> "Checking…"
                    FriendStatus.NO_CODE -> "Swap friend codes to see them online"
                },
                style = MaterialTheme.typography.bodySmall,
                color = if (friend.status == FriendStatus.ONLINE) Palette.success else Palette.secondary,
            )
        }
        when {
            friend.invitedUs -> Pill("Join", { friend.id?.let { id -> Core.act { acceptFriendInvite(id) } } })
            hosting && friend.status == FriendStatus.ONLINE -> Pill("Invite", { Core.act { inviteFriend(friend.index) } })
            else -> {}
        }
        IconAction(R.drawable.ic_close, "Remove", onRemove, tint = Palette.muted, size = 16.dp)
    }
}


@Composable
private fun CodeDialog(title: String, hint: String, onDone: (String) -> Unit, onCancel: () -> Unit) {
    var text by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onCancel,
        containerColor = Palette.panel,
        title = { Text(title, style = MaterialTheme.typography.titleLarge) },
        text = {
            OutlinedTextField(value = text, onValueChange = { text = it }, placeholder = { Text(hint) }, singleLine = true)
        },
        confirmButton = { TextButton(onClick = { onDone(text) }, enabled = text.isNotBlank()) { Text("Add") } },
        dismissButton = { TextButton(onClick = onCancel) { Text("Cancel") } },
    )
}

/** A friend's Jam invite, as a pop-up (when invites pop up). */
@Composable
fun InvitePopup() {
    val friends by Core.friends.collectAsStateWithLifecycle()
    val state = friends ?: return
    if (!state.invitePopups) return
    val invite = state.invites.firstOrNull() ?: return
    AlertDialog(
        onDismissRequest = { Core.act { declineFriendInvite(invite.id) } },
        containerColor = Palette.panel,
        title = { Text("${invite.name} invited you to their Jam", style = MaterialTheme.typography.titleLarge) },
        text = { Text("Listen together, in sync.", color = Palette.secondary) },
        confirmButton = {
            TextButton(onClick = { Core.act { acceptFriendInvite(invite.id) } }) { Text("Join", color = Color.White) }
        },
        dismissButton = {
            TextButton(onClick = { Core.act { declineFriendInvite(invite.id) } }) { Text("Not now") }
        },
    )
}

/** One of the user's devices asking to link (same Last.fm account). */
@Composable
fun DeviceLinkPopup() {
    val devices by Core.devices.collectAsStateWithLifecycle()
    val prompt = devices?.prompts?.firstOrNull() ?: return
    AlertDialog(
        onDismissRequest = { Core.act { declineDeviceLink(prompt.id) } },
        containerColor = Palette.panel,
        title = { Text("Link with ${prompt.name}?", style = MaterialTheme.typography.titleLarge) },
        text = { Text("Your playlists will stay in sync on both, whenever they're open.", color = Palette.secondary) },
        confirmButton = {
            TextButton(onClick = { Core.act { acceptDeviceLink(prompt.id) } }) { Text("Link", color = Color.White) }
        },
        dismissButton = {
            TextButton(onClick = { Core.act { declineDeviceLink(prompt.id) } }) { Text("Not now") }
        },
    )
}