import '../src/native.dart';

class OverviewUpdates extends NativeHandle {
  OverviewUpdates.internal(super.bridge, super.handle);

  Future<void> close() => closeNative();

  Future<String> next() async => await nextValue() as String;

  Future<void> refresh() async {
    await invoke('refresh');
  }
}
