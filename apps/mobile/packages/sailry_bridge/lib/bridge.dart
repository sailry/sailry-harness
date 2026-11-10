import 'dart:ffi';

import 'src/native.dart';

Future<void> initializeBridge({DynamicLibrary? library}) =>
    initializeNativeBridge(library: library);

Future<void> shutdownBridge() => shutdownNativeBridge();
