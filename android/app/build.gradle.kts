import java.io.File

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}

// One version for every platform: Cargo.toml is the source.
val version: String = File(rootDir.parentFile, "Cargo.toml").readLines()
    .first { it.startsWith("version = ") }
    .substringAfter('"').substringBefore('"')
val (major, minor, patch) = version.split('.').map { it.toInt() }

android {
    namespace = "io.github.liathy.musikpl"
    compileSdk = 37

    defaultConfig {
        applicationId = "io.github.liathy.musikpl"
        minSdk = 26
        targetSdk = 37
        versionCode = major * 10000 + minor * 100 + patch
        versionName = version
        ndk {
            // Phones; the Rust core and yt-dlp's Python are built for arm64.
            abiFilters += listOf("arm64-v8a")
        }
    }

    signingConfigs {
        create("release") {
            // The release key lives outside the repository: in
            // %LOCALAPPDATA%\MusiKPL (or where MUSIKPL_KEYSTORE says).
            val folder = File(System.getenv("LOCALAPPDATA") ?: "", "MusiKPL")
            val keystore = System.getenv("MUSIKPL_KEYSTORE")?.let(::File)
                ?: File(folder, "musikpl-android-release.jks")
            val password = System.getenv("MUSIKPL_KEYSTORE_PASSWORD")
                ?: File(folder, "android-keystore-password.txt").takeIf { it.exists() }?.readText()?.trim()
            if (keystore.exists() && password != null) {
                storeFile = keystore
                storePassword = password
                keyAlias = "musikpl"
                keyPassword = password
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            val release = signingConfigs.getByName("release")
            signingConfig = if (release.storeFile != null) release else signingConfigs.getByName("debug")
        }
        debug {
            // Test builds use the release key when it's here, so they install
            // over a released app (and back) without uninstalling.
            val release = signingConfigs.getByName("release")
            if (release.storeFile != null) signingConfig = release
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
        // NewPipeExtractor uses newer Java APIs than old Androids have.
        isCoreLibraryDesugaringEnabled = true
    }

    buildFeatures {
        compose = true
    }

    packaging {
        jniLibs {
            // yt-dlp's Python runs from the extracted native libraries.
            useLegacyPackaging = true
        }
    }
}

dependencies {
    coreLibraryDesugaring("com.android.tools:desugar_jdk_libs_nio:2.1.5")

    // The Rust core's Kotlin bindings call it through JNA.
    implementation("net.java.dev.jna:jna:5.19.1@aar")

    implementation(platform("androidx.compose:compose-bom:2026.09.00"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.activity:activity-compose:1.13.0")
    implementation("androidx.core:core-ktx:1.19.1")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.11.0")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.11.0")
    implementation("androidx.browser:browser:1.10.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.11.0")

    implementation("androidx.media3:media3-exoplayer:1.11.1")
    implementation("androidx.media3:media3-session:1.11.1")

    implementation("io.coil-kt.coil3:coil-compose:3.6.3")
    implementation("io.coil-kt.coil3:coil-network-okhttp:3.6.3")

    // YouTube: NewPipe first, yt-dlp when NewPipe can't.
    implementation("com.github.TeamNewPipe:NewPipeExtractor:v0.26.5")
    implementation("io.github.junkfood02.youtubedl-android:library:0.18.1")
    implementation("com.squareup.okhttp3:okhttp:4.12.0")
}
