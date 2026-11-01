package io.github.liathy.musikpl.update

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInstaller
import android.os.Build
import android.widget.Toast
import io.github.liathy.musikpl.core.Core
import java.io.File
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import uniffi.musikpl_mobile.DownloadProgress
import uniffi.musikpl_mobile.Update
import uniffi.musikpl_mobile.checkForUpdate
import uniffi.musikpl_mobile.downloadUpdate

sealed interface UpdateState {
    data object Idle : UpdateState
    data object Checking : UpdateState
    data object UpToDate : UpdateState
    data class Available(val update: Update) : UpdateState
    data class Downloading(val percent: Int) : UpdateState
    data object Installing : UpdateState
    data class Failed(val reason: String) : UpdateState
}

/**
 * Updates from MusiKPL's GitHub releases, like the desktop app: the core
 * looks GitHub up through Cloudflare's 1.1.1.1 and downloads the new APK;
 * Android's package installer puts it in place (asking the user once to
 * allow installing from MusiKPL). Data is kept, as the app is signed with
 * the same key.
 */
object Updater {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val _state = MutableStateFlow<UpdateState>(UpdateState.Idle)
    val state: StateFlow<UpdateState> = _state

    /** Look for a new version. `quiet`: on start, failures aren't shown. */
    fun check(quiet: Boolean = false) {
        if (_state.value is UpdateState.Checking || _state.value is UpdateState.Downloading) return
        _state.value = UpdateState.Checking
        scope.launch {
            _state.value = runCatching { checkForUpdate() }.fold(
                onSuccess = { update -> update?.let { UpdateState.Available(it) } ?: UpdateState.UpToDate },
                onFailure = { if (quiet) UpdateState.Idle else UpdateState.Failed(it.message ?: "Couldn't check for updates") },
            )
        }
    }

    fun install(context: Context, update: Update) {
        val app = context.applicationContext
        _state.value = UpdateState.Downloading(0)
        scope.launch {
            val file = File(app.cacheDir, "MusiKPL-${update.version}.apk")
            val downloaded = runCatching {
                downloadUpdate(update.url, file.absolutePath, object : DownloadProgress {
                    override fun progress(perMille: UInt) {
                        _state.value = UpdateState.Downloading(perMille.toInt() / 10)
                    }
                })
            }
            downloaded.exceptionOrNull()?.let {
                _state.value = UpdateState.Failed(it.message ?: "The download failed")
                return@launch
            }
            _state.value = UpdateState.Installing
            Core.save()
            runCatching { installApk(app, file) }.onFailure {
                _state.value = UpdateState.Failed(it.message ?: "Couldn't start the install")
            }
        }
    }

    private fun installApk(context: Context, apk: File) {
        val installer = context.packageManager.packageInstaller
        val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL)
        val id = installer.createSession(params)
        installer.openSession(id).use { session ->
            session.openWrite("MusiKPL.apk", 0, apk.length()).use { out ->
                apk.inputStream().use { it.copyTo(out) }
                session.fsync(out)
            }
            val flags = PendingIntent.FLAG_UPDATE_CURRENT or
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) PendingIntent.FLAG_MUTABLE else 0
            val result = PendingIntent.getBroadcast(
                context,
                id,
                Intent(context, InstallReceiver::class.java),
                flags,
            )
            session.commit(result.intentSender)
        }
    }

    internal fun failed(reason: String) {
        _state.value = UpdateState.Failed(reason)
    }
}

/** The package installer's answer: ask the user to confirm, or report. */
class InstallReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        when (val status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)) {
            PackageInstaller.STATUS_PENDING_USER_ACTION -> {
                @Suppress("DEPRECATION")
                val confirm = if (Build.VERSION.SDK_INT >= 33) {
                    intent.getParcelableExtra(Intent.EXTRA_INTENT, Intent::class.java)
                } else {
                    intent.getParcelableExtra(Intent.EXTRA_INTENT)
                }
                confirm?.let { context.startActivity(it.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) }
            }
            PackageInstaller.STATUS_SUCCESS -> {}
            else -> {
                val message = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE)
                    ?: "The update wasn't installed (code $status)"
                Updater.failed(message)
                Toast.makeText(context, message, Toast.LENGTH_LONG).show()
            }
        }
    }
}
