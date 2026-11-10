import groovy.json.JsonSlurper

plugins {
    id("com.android.application")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
}

val releaseSigning = listOf(
    "ANDROID_KEYSTORE_PATH", "ANDROID_STORE_PASSWORD", "ANDROID_KEY_ALIAS", "ANDROID_KEY_PASSWORD"
).associateWith { System.getenv(it) }
val hasReleaseSigning = releaseSigning.values.all { !it.isNullOrBlank() }
require(releaseSigning.values.all { it.isNullOrBlank() } || hasReleaseSigning) {
    "Android release signing configuration is incomplete"
}
require(System.getenv("SAILRY_ANDROID_RELEASE") != "1" || hasReleaseSigning) {
    "Android distribution requires release signing credentials"
}

android {
    namespace = "com.sailry.sailry_mobile"
    compileSdk = flutter.compileSdkVersion
    ndkVersion = flutter.ndkVersion

    sourceSets.getByName("main").jniLibs.srcDir(rootProject.file("../../../.runtime/native/android-jni"))

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    defaultConfig {
        applicationId = "com.sailry.sailry_mobile"
        minSdk = flutter.minSdkVersion
        targetSdk = flutter.targetSdkVersion
        versionCode = flutter.versionCode
        versionName = flutter.versionName
        proguardFiles("consumer-rules.pro")
        // Flutter's default filters also admit other ABIs from dependency AARs.
        if (providers.gradleProperty("target-platform").orNull == "android-arm64") {
            ndk {
                abiFilters.clear()
                abiFilters.add("arm64-v8a")
            }
        }
    }

    signingConfigs {
        if (hasReleaseSigning) {
            create("distribution") {
                storeFile = file(releaseSigning.getValue("ANDROID_KEYSTORE_PATH")!!)
                storePassword = releaseSigning.getValue("ANDROID_STORE_PASSWORD")
                keyAlias = releaseSigning.getValue("ANDROID_KEY_ALIAS")
                keyPassword = releaseSigning.getValue("ANDROID_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            // Local and CI builds keep their disposable signer; tag builds require distribution credentials.
            signingConfig = signingConfigs.getByName(if (hasReleaseSigning) "distribution" else "debug")
        }
    }
}

// Discover the companion from the locked Rust graph, as upstream recommends.
val rustMetadata = providers.exec {
    workingDir(rootProject.file("../../.."))
    commandLine("rustup", "run", "1.98.0", "cargo", "metadata", "--locked", "--format-version", "1",
        "--manifest-path", "native/iroh/Cargo.toml",
        "--filter-platform", "aarch64-linux-android")
}.standardOutput.asText
val packages = (JsonSlurper().parseText(rustMetadata.get()) as Map<*, *>)["packages"] as List<*>
val verifier = packages.map { it as Map<*, *> }.first { it["name"] == "rustls-platform-verifier-android" }
repositories {
    maven {
        url = uri(File(File(verifier["manifest_path"] as String).parentFile, "maven"))
    }
}
dependencies {
    implementation("rustls:rustls-platform-verifier:${verifier["version"]}")
}

val nativePlatforms = providers.gradleProperty("target-platform").orNull
    ?.split(",") ?: listOf("android-arm", "android-arm64", "android-x64")
val nativeAbis = nativePlatforms.map {
    when (it) {
        "android-arm" -> "armeabi-v7a"
        "android-arm64" -> "arm64-v8a"
        "android-x64" -> "x86_64"
        else -> error("Unsupported Sailry Mobile target: $it")
    }
}
val prepareSailryNative = tasks.register("prepareSailryNative") {
    val sourceRoot = rootProject.file("../../..")
    inputs.files(fileTree(File(sourceRoot, "internal/client")), fileTree(File(sourceRoot, "internal/link")),
        fileTree(File(sourceRoot, "internal/protocol")), fileTree(File(sourceRoot, "internal/mobile")),
        fileTree(File(sourceRoot, "cmd/sailry-mobile")), fileTree(File(sourceRoot, "native/iroh/src")),
        File(sourceRoot, "go.mod"), File(sourceRoot, "go.sum"),
        File(sourceRoot, "native/iroh/Cargo.toml"), File(sourceRoot, "native/iroh/Cargo.lock"))
    inputs.property("abis", nativeAbis.joinToString(","))
    inputs.property("ndkVersion", flutter.ndkVersion)
    outputs.files(nativeAbis.flatMap { abi ->
        listOf(File(sourceRoot, ".runtime/native/android-jni/$abi/libsailry_mobile.so"),
            File(sourceRoot, ".runtime/native/android-jni/$abi/libsailry_iroh.so"))
    })
    doLast {
        nativeAbis.forEach { abi ->
            providers.exec {
                workingDir(sourceRoot)
                environment("ANDROID_NDK_HOME", androidComponents.sdkComponents.ndkDirectory.get().asFile.absolutePath)
                environment("SAILRY_ANDROID_API", android.defaultConfig.minSdk.toString())
                commandLine("sh", "cmd/sailry-mobile/build.sh", "android", abi)
            }.result.get().assertNormalExitValue()
        }
    }
}
tasks.named("preBuild").configure { dependsOn(prepareSailryNative) }

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
    }
}

flutter {
    source = "../.."
}
