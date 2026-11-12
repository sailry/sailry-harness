import 'dart:convert';

import '../src/native.dart';

class Digest extends NativeHandle {
  Digest.internal(super.bridge, super.handle);

  static Future<Digest> newInstance() async {
    final bridge = nativeBridge;
    final value = await bridge.invoke(0, 'new', {'kind': 'digest'});
    return Digest.internal(bridge, nativeHandle(value, 'digest'));
  }

  Future<void> update({required List<int> bytes}) async {
    await invoke('update', {'bytes': base64Encode(bytes)});
  }

  Future<String> revision() async => await invoke('revision') as String;
}
