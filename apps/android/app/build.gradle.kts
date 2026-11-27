import java.security.MessageDigest

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

// The Rust workspace is two levels up. Builds for the phone and for this machine share one
// target folder of their own, apart from desktop builds.
val repoRoot: File = rootDir.resolve("../..").canonicalFile
val rustTargetDir: File = repoRoot.resolve("target/android")
val rustTriples = mapOf("arm64-v8a" to "aarch64-linux-android", "x86_64" to "x86_64-linux-android")
val phoneAbis = rustTriples.keys.toList()

// rustls-platform-verifier's Kotlin half, vendored in libs/ (see its README). It must be the
// version Cargo.lock pairs with the Rust half, or certificate checks fail at run time, and its
// content must be the one recorded here.
val platformVerifierSha256 = mapOf("0.2.0" to "aa021794230fbc2f0be355999e2cf67398dd563de066a2e17001ffbd0b69101b")
val platformVerifierAar: File = run {
    val lockFile = providers.fileContents(layout.projectDirectory.file(repoRoot.resolve("Cargo.lock").path)).asText.get()
    val version = Regex("""name = "rustls-platform-verifier-android"\nversion = "([^"]+)"""").find(lockFile)?.groupValues?.get(1)
        ?: error("rustls-platform-verifier-android isn't in Cargo.lock")
    val aar = file("libs/rustls-platform-verifier-$version.aar")
    check(aar.exists()) { "Cargo.lock wants rustls-platform-verifier-android $version: vendor its AAR as $aar (see libs/README.md)" }
    val expected = platformVerifierSha256[version] ?: error("record the SHA-256 of $aar in platformVerifierSha256 (see libs/README.md)")
    val bytes = providers.fileContents(layout.projectDirectory.file(aar.path)).asBytes.get()
    val actual = MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }
    check(actual == expected) { "$aar has SHA-256 $actual, not the recorded $expected" }
    aar
}

android {
    namespace = "dev.shynewt.shyspace"
    // Compose needs the 37.2 APIs to compile against; the app still targets 37.
    compileSdk {
        version = release(37) { minorApiLevel = 2 }
    }
    buildToolsVersion = "37.0.0"
    // The NDK in the build image; packaging strips the core's debug symbols with it.
    ndkVersion = "29.0.14206865"

    defaultConfig {
        applicationId = "dev.shynewt.shyspace"
        minSdk = 29
        targetSdk = 37
        versionCode = providers.gradleProperty("versionCode").orNull?.toInt() ?: 1
        versionName = providers.gradleProperty("versionName").orNull ?: "dev"
        // The core is built for these only; JNA alone would let 32-bit phones install an app
        // that can't start.
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    signingConfigs {
        // A fixed, public debug key, so builds from fresh containers install as updates.
        getByName("debug") {
            storeFile = rootProject.file("debug.keystore")
            storePassword = "android"
            keyAlias = "androiddebugkey"
            keyPassword = "android"
        }
    }

    buildTypes {
        debug {
            applicationIdSuffix = ".debug"
            versionNameSuffix = "-debug"
            signingConfig = signingConfigs.getByName("debug")
        }
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    lint {
        abortOnError = true
        // Version-bump hints change with every upstream release, not with this code.
        disable += setOf("NewerVersionAvailable", "GradleDependency", "AndroidGradlePluginVersion")
    }

    testOptions {
        unitTests.isIncludeAndroidResources = true
    }
}

// Release unit tests prove the release bindings lack the debug-only parts.
androidComponents {
    beforeVariants(selector().withBuildType("release")) { variant ->
        variant.hostTests[com.android.build.api.variant.HostTestBuilder.UNIT_TEST_TYPE]?.enable = true
    }
}

kotlin {
    compilerOptions {
        optIn.addAll(
            "androidx.compose.material3.ExperimentalMaterial3Api",
            "androidx.compose.material3.ExperimentalMaterial3ExpressiveApi",
        )
    }
}

/**
 * Builds one build type's core: the phone libraries, and a library for this machine that the
 * Kotlin bindings are generated from and the JVM tests load. Debug builds add the in-memory
 * bucket for tests and demos; the bindings always match the library they call.
 */
fun registerRustCore(buildType: String, isRelease: Boolean, features: List<String>) {
    val name = buildType.replaceFirstChar { it.uppercase() }
    val profileArgs = if (isRelease) listOf("--release") else emptyList()
    val profile = if (isRelease) "release" else "debug"
    val hostDir = rustTargetDir.resolve(profile)
    val jniLibsDir = layout.buildDirectory.dir("rust/$buildType/jniLibs").get().asFile
    val bindingsDir = layout.buildDirectory.dir("generated/uniffi/$buildType").get().asFile

    val host = tasks.register<Exec>("buildRustHost$name") {
        workingDir(repoRoot)
        environment("CARGO_TARGET_DIR", rustTargetDir)
        val hostFeatures = (features + listOf("bindgen", "vendored-openssl")).joinToString(",")
        commandLine(listOf("cargo", "build", "-p", "shy-ffi", "--lib", "--bins", "--features", hostFeatures) + profileArgs)
    }
    val generate = tasks.register<Exec>("generateRustBindings$name") {
        dependsOn(host)
        // The generator reads each crate's uniffi.toml through cargo metadata.
        workingDir(repoRoot)
        environment("CARGO_TARGET_DIR", rustTargetDir)
        // Declared, or Gradle would call the bindings current while the core changes.
        inputs.file(hostDir.resolve("libshy_ffi.so"))
        inputs.files(repoRoot.resolve("crates/shy-ffi/uniffi.toml"), repoRoot.resolve("crates/shy-core/uniffi.toml")).optional()
        outputs.dir(bindingsDir)
        commandLine(
            hostDir.resolve("uniffi-bindgen"), "generate",
            "--library", hostDir.resolve("libshy_ffi.so"),
            "--language", "kotlin", "--no-format", "--out-dir", bindingsDir,
        )
    }
    val phone = tasks.register<Exec>("buildRustAndroid$name") {
        workingDir(repoRoot)
        environment("CARGO_TARGET_DIR", rustTargetDir)
        val phoneFeatures = (features + "vendored-openssl").joinToString(",")
        val targets = phoneAbis.flatMap { listOf("-t", it) }
        commandLine(
            listOf("cargo", "ndk") + targets + listOf("-P", "29", "build", "-p", "shy-ffi", "--lib", "--features", phoneFeatures) + profileArgs,
        )
    }
    // Some dependencies also build libraries of their own; the core is all in libshy_ffi.so.
    val copy = tasks.register<Sync>("copyRustAndroid$name") {
        dependsOn(phone)
        into(jniLibsDir)
        rustTriples.forEach { (abi, triple) ->
            from(rustTargetDir.resolve("$triple/$profile/libshy_ffi.so")) { into(abi) }
        }
    }

    android.sourceSets.getByName(buildType) {
        kotlin.directories.add(bindingsDir.path)
        jniLibs.directories.add(jniLibsDir.path)
    }
    tasks.matching { it.name == "pre${name}Build" }.configureEach { dependsOn(generate, copy) }
    tasks.matching { it.name == "test${name}UnitTest" }.configureEach {
        dependsOn(host)
        (this as Test).systemProperty("jna.library.path", hostDir.path)
    }
}

registerRustCore("debug", isRelease = false, features = listOf("memory-bucket"))
registerRustCore("release", isRelease = true, features = emptyList())

// Robolectric's Android runtime, fetched by Gradle so it's cached and checked against
// verification-metadata.xml like every other download; Robolectric then runs offline from it
// instead of fetching it itself during the tests.
val robolectricRuntime: Configuration = configurations.create("robolectricRuntime")
val robolectricJars = tasks.register<Sync>("robolectricJars") {
    from(robolectricRuntime)
    into(layout.buildDirectory.dir("robolectric-jars"))
}

tasks.withType<Test>().configureEach {
    dependsOn(robolectricJars)
    systemProperty("robolectric.offline", "true")
    systemProperty("robolectric.dependency.dir", layout.buildDirectory.dir("robolectric-jars").get().asFile.path)
    testLogging {
        events("failed")
        showStandardStreams = true
    }
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.navigation3)
    implementation(libs.androidx.navigation3.runtime)
    implementation(libs.androidx.navigation3.ui)

    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.graphics)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.compose.material3)
    debugImplementation(libs.compose.ui.tooling)

    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.jna) { artifact { type = "aar" } }
    implementation(files(platformVerifierAar))

    testImplementation(libs.junit)
    testImplementation(libs.kotlinx.coroutines.test)
    testImplementation(libs.jna)
    testImplementation(libs.robolectric)
    robolectricRuntime(libs.robolectric.android.all)
    testImplementation(libs.androidx.test.junit)
    testImplementation(platform(libs.compose.bom))
    testImplementation(libs.compose.ui.test.junit4)
    debugImplementation(libs.compose.ui.test.manifest)

    androidTestImplementation(libs.androidx.test.junit)
    androidTestImplementation(libs.androidx.test.runner)
}
