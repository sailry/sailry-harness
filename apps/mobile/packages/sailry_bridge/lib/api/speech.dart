import '../src/native.dart';

class SpeechInput extends NativeHandle {
  SpeechInput.internal(super.bridge, super.handle);

  static Future<SpeechInput> newInstance({required String directory}) async {
    final bridge = nativeBridge;
    final value = await bridge.invoke(0, 'new', {
      'kind': 'speech_input',
      'directory': directory,
    });
    return SpeechInput.internal(bridge, nativeHandle(value, 'speech_input'));
  }

  Future<bool> ready() async => await invoke('ready') as bool;

  Future<void> download() async {
    await invoke('download');
  }

  Future<int> downloadProgress() async =>
      await invoke('download_progress') as int;

  Future<void> cancel() async {
    await invoke('cancel');
  }

  Future<String> transcribe({
    required List<double> samples,
    required int sampleRate,
    required String language,
  }) async =>
      await invoke('transcribe', {
            'samples': samples,
            'sample_rate': sampleRate,
            'language': language,
          })
          as String;
}
