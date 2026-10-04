import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'frb_generated.dart';

Future<void>? _initialization;

/// One native library per Dart isolate, shared by all controller connections.
Future<void> initializeBridge({ExternalLibrary? library}) {
  if (RustLib.instance.initialized) return Future.value();
  return _initialization ??= RustLib.init(externalLibrary: library).whenComplete(() {
    _initialization = null;
  });
}
