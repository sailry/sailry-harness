import 'dart:async';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:record/record.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/speech.dart';
import '../../../runtime/session.dart';
import '../../../ui/kit.dart';

Future<void> dictate(BuildContext context, TextEditingController draft) async {
  final speech = AppSession.of(context).speech;
  if (speech == null) return;
  final text = await showAppSheet<String>(
    context,
    context.tr('voice'),
    child: VoiceInput(speech: speech),
  );
  if (!context.mounted || text == null || text.isEmpty) return;
  final selection = draft.selection;
  final start = selection.isValid ? selection.start : draft.text.length;
  final end = selection.isValid ? selection.end : draft.text.length;
  draft.value = TextEditingValue(
    text: draft.text.replaceRange(start, end, text),
    selection: TextSelection.collapsed(offset: start + text.length),
  );
}

class VoiceInput extends StatefulWidget {
  const VoiceInput({super.key, required this.speech});
  final SpeechController speech;
  @override
  State<VoiceInput> createState() => _VoiceInputState();
}

class _VoiceInputState extends State<VoiceInput> {
  final _recorder = AudioRecorder();
  final _bytes = BytesBuilder(copy: false);
  StreamSubscription<Uint8List>? _subscription;
  bool _recording = false;
  bool _working = false;
  bool _transcribing = false;
  String? _error;

  @override
  void dispose() {
    unawaited(_subscription?.cancel());
    unawaited(_recorder.dispose());
    if (_transcribing) unawaited(widget.speech.input.cancel());
    super.dispose();
  }

  Future<void> _start() async {
    if (_working || _recording) return;
    setState(() {
      _working = true;
      _error = null;
    });
    try {
      if (!await _recorder.hasPermission()) {
        throw StateError(context.tr('conversationMicrophoneDenied'));
      }
      if (!mounted) return;
      final stream = await _recorder.startStream(
        const RecordConfig(
          encoder: AudioEncoder.pcm16bits,
          sampleRate: 16000,
          numChannels: 1,
        ),
      );
      if (!mounted) {
        await _recorder.stop();
        return;
      }
      _bytes.clear();
      _subscription = stream.listen(
        (bytes) {
          // Matches the shared Rust recognizer's five-minute PCM limit.
          const limit = 16000 * 300 * 2;
          final remaining = limit - _bytes.length;
          _bytes.add(
            bytes.length <= remaining
                ? bytes
                : Uint8List.sublistView(bytes, 0, remaining),
          );
          if (_bytes.length == limit) unawaited(_finish());
        },
        onError: (Object error) {
          if (mounted) {
            setState(() {
              _error = context.tr('conversationRecordingFailed');
              _recording = false;
            });
          }
          unawaited(_recorder.stop());
        },
      );
      setState(() => _recording = true);
    } catch (_) {
      if (mounted) {
        setState(() => _error = context.tr('conversationMicrophoneDenied'));
      }
    } finally {
      if (mounted) setState(() => _working = false);
    }
  }

  Future<void> _finish() async {
    if (!_recording || _working) return;
    setState(() {
      _working = true;
      _recording = false;
    });
    try {
      await _recorder.stop();
      await _subscription?.cancel();
      _subscription = null;
      final bytes = _bytes.takeBytes();
      final data = ByteData.sublistView(bytes);
      final samples = List<double>.generate(
        bytes.length ~/ 2,
        (index) => data.getInt16(index * 2, Endian.little) / 32768,
      );
      _transcribing = true;
      final text = await widget.speech.input.transcribe(
        samples: samples,
        sampleRate: 16000,
        language: widget.speech.language,
      );
      if (mounted) Navigator.pop(context, text);
    } catch (_) {
      if (mounted) {
        setState(() => _error = context.tr('conversationRecordingFailed'));
      }
    } finally {
      _transcribing = false;
      if (mounted) setState(() => _working = false);
    }
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: widget.speech,
    builder: (context, _) => Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (!widget.speech.enabled)
          Text(context.tr('conversationSpeechDisabled'))
        else if (!widget.speech.ready)
          Text(context.tr('conversationSpeechMissing'))
        else
          Text(
            context.tr(
              _recording
                  ? 'conversationRecording'
                  : _working
                  ? 'conversationTranscribing'
                  : 'conversationRecordReady',
            ),
          ),
        if (_error != null)
          Padding(
            padding: const EdgeInsets.only(top: 12),
            child: Text(
              _error!,
              style: TextStyle(color: Theme.of(context).colorScheme.error),
            ),
          ),
        const SizedBox(height: 16),
        FilledButton(
          onPressed: _working || !widget.speech.enabled || !widget.speech.ready
              ? null
              : _recording
              ? _finish
              : _start,
          child: Text(
            context.tr(
              _recording
                  ? 'conversationFinishRecording'
                  : 'conversationStartRecording',
            ),
          ),
        ),
      ],
    ),
  );
}
