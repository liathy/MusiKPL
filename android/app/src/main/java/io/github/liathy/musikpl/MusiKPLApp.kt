package io.github.liathy.musikpl

import android.app.Application
import io.github.liathy.musikpl.core.Core

class MusiKPLApp : Application() {
    override fun onCreate() {
        super.onCreate()
        Core.init(this)
        io.github.liathy.musikpl.update.Updater.check(quiet = true)
    }
}
