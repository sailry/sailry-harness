import '../../src/native.dart';

class Upload extends NativeHandle {
  Upload.internal(super.bridge, super.handle);

  Future<void> close() => closeNative();

  Future<void> write({required List<int> bytes}) async {
    await invoke('write', {'bytes': bytes});
  }

  Future<void> finish() async {
    await invoke('finish');
  }
}
