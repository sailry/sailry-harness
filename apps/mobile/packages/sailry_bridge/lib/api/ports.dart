import '../src/native.dart';

class Forwarding extends NativeHandle {
  Forwarding.internal(super.bridge, super.handle);

  Future<void> close() => closeNative();

  Future<int> localPort() async => await invoke('local_port') as int;

  Future<String> next() async => await nextValue() as String;
}
