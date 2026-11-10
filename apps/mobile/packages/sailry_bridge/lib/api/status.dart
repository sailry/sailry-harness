import '../src/native.dart';

class StatusUpdates extends NativeHandle {
  StatusUpdates.internal(super.bridge, super.handle);

  Future<void> close() => closeNative();

  Future<String> next() async => await nextValue() as String;
}
