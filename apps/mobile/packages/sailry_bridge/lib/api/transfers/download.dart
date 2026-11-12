import 'dart:convert';
import 'dart:typed_data';

import '../../src/native.dart';

class Download extends NativeHandle {
  Download.internal(super.bridge, super.handle);

  Future<void> close() => closeNative();

  Future<Uint8List?> next() async {
    final value = await nextValue();
    if (value == null) return null;
    return base64Decode(value as String);
  }
}
