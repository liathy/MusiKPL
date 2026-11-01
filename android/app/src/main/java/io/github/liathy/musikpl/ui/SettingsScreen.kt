package io.github.liathy.musikpl.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
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
import io.github.liathy.musikpl.core.Core
import io.github.liathy.musikpl.update.UpdateState
import io.github.liathy.musikpl.update.Updater

@Composable
fun SettingsScreen(nav: Nav) {
    val settings by Core.settings.collectAsStateWithLifecycle()
    val state = settings ?: return Loading()
    val context = LocalContext.current
    var clearing by remember { mutableStateOf(false) }
    val version = remember {
        runCatching { context.packageManager.getPackageInfo(context.packageName, 0).versionName }.getOrNull()
    }

    Column(Modifier.verticalScroll(rememberScrollState()).padding(bottom = 24.dp)) {
        Text("Settings", style = MaterialTheme.typography.headlineMedium, modifier = Modifier.padding(16.dp))

        Card("Last.fm") {
            if (state.lastfmUser.isNotEmpty()) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    RoundPhoto(state.lastfmAvatar, state.lastfmUser, Modifier.size(48.dp))
                    Spacer(Modifier.width(12.dp))
                    Column(Modifier.weight(1f)) {
                        Text(state.lastfmUser, style = MaterialTheme.typography.titleMedium)
                        Text(
                            state.lastfmScrobbles?.let { "${compact(it)} scrobbles" } ?: "Connected",
                            style = MaterialTheme.typography.bodySmall,
                            color = Palette.secondary,
                        )
                    }
                }
                Spacer(Modifier.size(12.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Pill("Your profile", { nav.profile(null) })
                    Pill("Disconnect", { Core.act { disconnectLastfm() } }, primary = false)
                }
            } else if (state.lastfmConnecting) {
                Text(
                    "Approve MusiKPL on the Last.fm page in your browser, then come back here.",
                    color = Palette.secondary,
                )
                Spacer(Modifier.size(12.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    state.lastfmApprovalUrl?.let { url ->
                        Pill("Open the page again", { Core.act { openLink(url) } })
                    }
                    Pill("Cancel", { Core.act { cancelLastfmConnect() } }, primary = false)
                }
            } else {
                Text(
                    "Scrobble what you play, love songs you ♥, and see your profile and stats.",
                    color = Palette.secondary,
                )
                Spacer(Modifier.size(12.dp))
                if (state.lastfmOneClick) {
                    Pill("Connect with Last.fm", { Core.act { connectLastfm() } })
                } else {
                    Text("This build has no Last.fm key.", color = Palette.muted)
                }
            }
        }

        Card("Playback") {
            Toggle(
                "Auto Mix",
                "Keeps the queue filled with songs like the one playing.",
                state.autoMix,
            ) { enabled -> Core.act { setAutoMix(enabled) } }
            Spacer(Modifier.size(12.dp))
            Toggle(
                "Jam invites as pop-ups",
                "Off: friends' invites wait quietly on the Jam tab.",
                state.invitePopups,
            ) { enabled -> Core.act { setInvitePopups(enabled) } }
        }

        Card("Updates") {
            val update by Updater.state.collectAsStateWithLifecycle()
            when (val current = update) {
                UpdateState.Idle, UpdateState.UpToDate, is UpdateState.Failed -> {
                    Text(
                        when (current) {
                            UpdateState.UpToDate -> "MusiKPL is up to date."
                            is UpdateState.Failed -> current.reason
                            else -> "New versions come from MusiKPL's GitHub releases."
                        },
                        color = if (current is UpdateState.Failed) Palette.danger else Palette.secondary,
                    )
                    Spacer(Modifier.size(8.dp))
                    Pill("Check for updates", { Updater.check() }, primary = false)
                }
                UpdateState.Checking -> Text("Checking…", color = Palette.secondary)
                is UpdateState.Available -> {
                    Text("MusiKPL ${current.update.version} is available.")
                    Spacer(Modifier.size(8.dp))
                    Pill("Install update", { Updater.install(context, current.update) })
                }
                is UpdateState.Downloading -> Text("Downloading update… ${current.percent}%", color = Palette.secondary)
                UpdateState.Installing -> Text("Installing… confirm on the screen Android shows.", color = Palette.secondary)
            }
        }

        Card("Your taste") {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Pill("Pick genres", {
                    Core.act { pickGenres() }
                    nav.goTo(Tab.Home)
                }, primary = false)
                Pill("Clear history", { clearing = true }, primary = false)
            }
        }

        state.error?.let { error ->
            Text(error, color = Palette.danger, modifier = Modifier.padding(16.dp))
        }

        Card("About") {
            Text("MusiKPL ${version.orEmpty()}", style = MaterialTheme.typography.titleMedium)
            Text(
                "Free software under the GNU GPL v3.0 or later.",
                style = MaterialTheme.typography.bodySmall,
                color = Palette.secondary,
            )
            Spacer(Modifier.size(8.dp))
            Pill("GitHub", { Core.act { openLink("https://github.com/liathy/MusiKPL") } }, primary = false)
        }
    }

    if (clearing) {
        AlertDialog(
            onDismissRequest = { clearing = false },
            containerColor = Palette.panel,
            title = { Text("Clear your listening history?", style = MaterialTheme.typography.titleLarge) },
            text = { Text("What you listened to and liked is forgotten, and Home starts over. Playlists stay.") },
            confirmButton = {
                TextButton(onClick = {
                    clearing = false
                    Core.act { clearHistory() }
                }) { Text("Clear", color = Palette.danger) }
            },
            dismissButton = { TextButton(onClick = { clearing = false }) { Text("Cancel") } },
        )
    }
}

@Composable
private fun Card(title: String, content: @Composable () -> Unit) {
    Column(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 6.dp)
            .clip(RoundedCornerShape(12.dp))
            .background(Palette.panel)
            .padding(16.dp),
    ) {
        Text(title.uppercase(), style = MaterialTheme.typography.bodySmall, color = Palette.muted)
        Spacer(Modifier.size(8.dp))
        content()
    }
}

@Composable
private fun Toggle(title: String, detail: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    Row(
        Modifier.fillMaxWidth().clickable { onChange(!checked) },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.titleMedium)
            Text(detail, style = MaterialTheme.typography.bodySmall, color = Palette.secondary)
        }
        Switch(
            checked = checked,
            onCheckedChange = onChange,
            colors = SwitchDefaults.colors(
                checkedThumbColor = Color.Black,
                checkedTrackColor = Color.White,
                uncheckedThumbColor = Palette.secondary,
                uncheckedTrackColor = Palette.surface,
            ),
        )
    }
}
