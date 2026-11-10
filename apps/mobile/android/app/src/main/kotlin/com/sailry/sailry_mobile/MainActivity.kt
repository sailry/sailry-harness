package com.sailry.sailry_mobile

import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import ai.sailry.bridge.Native

class MainActivity : FlutterActivity() {
    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        Native.initialize(applicationContext)
        super.configureFlutterEngine(flutterEngine)
    }
}
