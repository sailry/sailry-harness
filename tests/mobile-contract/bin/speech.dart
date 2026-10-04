import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api/speech.dart';
import 'package:sailry_bridge/frb_generated.dart';

// This contract exercises the actual Dart/Rust ABI without Flutter pages or a Node.
Future<void> main() async {
  final env = Platform.environment;
  await RustLib.init(
    externalLibrary: ExternalLibrary.open(env['SAILRY_BRIDGE_LIBRARY']!),
  );
  final speech = await SpeechInput.newInstance(
    directory: env['SAILRY_SPEECH_PROFILE']!,
  );
  try {
    if (await speech.downloadProgress() != 0) {
      throw StateError('unexpected initial progress');
    }
    final fixture = env['SAILRY_SPEECH_PCM'];
    if (fixture == null) {
      if (await speech.ready()) throw StateError('empty profile is ready');
      final text = await speech.transcribe(
        samples: List<double>.filled(16000, 0),
        sampleRate: 16000,
        language: 'auto',
      );
      if (text.isNotEmpty) throw StateError('silence produced a transcript');
    } else {
      if (!await speech.ready()) throw StateError('fixture model is not ready');
      await speech.download();
      if (await speech.downloadProgress() != 100) {
        throw StateError('validated model did not complete preparation');
      }
      final input = jsonDecode(await File(fixture).readAsString()) as Map;
      final text = await speech.transcribe(
        samples: (input['samples'] as List)
            .cast<num>()
            .map((n) => n.toDouble())
            .toList(),
        sampleRate: input['rate'] as int,
        language: 'auto',
      );
      if (!text.toLowerCase().contains(input['expected'] as String)) {
        throw StateError('unexpected transcript: $text');
      }
      stdout.writeln(text);
    }
    try {
      await speech.transcribe(samples: [0], sampleRate: 0, language: 'auto');
      throw StateError('invalid sample rate accepted');
    } catch (error) {
      if (!error.toString().contains('invalid speech audio')) rethrow;
    }
    await speech.cancel();
    stdout.writeln('Speech FFI contract passed');
  } finally {
    speech.dispose();
    RustLib.dispose();
  }
}
