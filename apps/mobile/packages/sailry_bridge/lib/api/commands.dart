import '../src/native.dart';

class CommandUpdates extends NativeHandle {
  CommandUpdates.internal(super.bridge, super.handle);

  Future<void> close() => closeNative();

  Future<String> next() async => await nextValue() as String;
}

class CommandDetail extends NativeHandle {
  CommandDetail.internal(super.bridge, super.handle);

  Future<String> read() async => await invoke('read') as String;
}
