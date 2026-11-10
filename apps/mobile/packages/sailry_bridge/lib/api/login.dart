import '../src/native.dart';

class LoginUpdates extends NativeHandle {
  LoginUpdates.internal(super.bridge, super.handle);

  Future<void> close() => closeNative();

  Future<String> next() async => await nextValue() as String;
}
