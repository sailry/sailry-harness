import '../src/native.dart';

class ConversationUpdates extends NativeHandle {
  ConversationUpdates.internal(super.bridge, super.handle);

  Future<void> close() => closeNative();

  Future<String> next() async => await nextValue() as String;

  Future<void> loadOlder() async {
    await invoke('load_older');
  }

  Future<void> loadThrough({required BigInt sequence}) async {
    await invoke('load_through', {'sequence': sequence.toString()});
  }

  Future<void> trackTools({required String selection}) async {
    await invoke('track_tools', {'selection': selection});
  }
}
