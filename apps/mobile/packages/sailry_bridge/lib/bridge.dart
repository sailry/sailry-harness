import 'dart:ffi';

import 'src/native.dart';

export 'src/envelope.dart' show BridgeFailure;

Future<void> initializeBridge({DynamicLibrary? library}) =>
    initializeNativeBridge(library: library);
