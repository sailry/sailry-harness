import groovy.json.JsonSlurper

plugins {
    id("com.android.application")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
}

android {
    namespace = "com.sailry.sailry_mobile"
    compileSdk = flutter.compileSdkVersion
    ndkVersion = flutter.ndkVersion

    sourceSets.getByName("main").java.srcDir("../../../../crates/mobile-bridge/android/src/main/java")

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    defaultConfig {
        // TODO: Specify your own unique Application ID (https://developer.android.com/studio/build/application-id.html).
        applicationId = "com.sailry.sailry_mobile"
        // You can update the following values to match your application needs.
        // For more information, see: https://flutter.dev/to/review-gradle-config.
        minSdk = flutter.minSdkVersion
        targetSdk = flutter.targetSdkVersion
        versionCode = flutter.versionCode
        versionName = flutter.versionName
        proguardFiles("../../../../crates/mobile-bridge/android/consumer-rules.pro")
    }

    buildTypes {
        release {
            // TODO: Add your own signing config for the release build.
            // Signing with the debug keys for now, so `flutter run --release` works.
            signingConfig = signingConfigs.getByName("debug")
        }
    }
}

// Discover the companion from the locked Rust graph, as upstream recommends.
val rustMetadata = providers.exec {
    workingDir(rootProject.file("../../.."))
    commandLine("cargo", "metadata", "--locked", "--format-version", "1",
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

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
    }
}

flutter {
    source = "../.."
}
