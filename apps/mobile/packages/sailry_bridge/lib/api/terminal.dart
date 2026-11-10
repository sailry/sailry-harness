import '../src/native.dart';

class TerminalUpdates extends NativeHandle {
  TerminalUpdates.internal(super.bridge, super.handle);

  Future<void> close() => closeNative();

  Future<String> next() async => await nextValue() as String;
}
