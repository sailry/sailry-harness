import '../src/native.dart';

class CommandUpdates extends NativeHandle {
  CommandUpdates.internal(super.bridge, super.handle);

  Future<void> close() => closeNative();

  Future<String> next() async => await nextValue() as String;
}
