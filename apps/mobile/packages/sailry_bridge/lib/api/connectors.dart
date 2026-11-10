import '../src/native.dart';

class ConnectionsUpdates extends NativeHandle {
  ConnectionsUpdates.internal(super.bridge, super.handle);

  Future<void> close() => closeNative();

  Future<String> next() async => await nextValue() as String;
}
