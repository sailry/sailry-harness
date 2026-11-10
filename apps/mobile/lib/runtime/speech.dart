import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:sailry_bridge/api/speech.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// Controller-local model lifetime shared by settings and microphone capture.
class SpeechController extends ChangeNotifier {
  SpeechController(
    this.directory, {
    Future<SpeechInput> Function(String)? open,
    SharedPreferencesAsync? preferences,
  }) : _open = open ?? _openInput,
       _preferences = preferences ?? SharedPreferencesAsync();

  final String directory;
  final Future<SpeechInput> Function(String) _open;
  final SharedPreferencesAsync _preferences;
  static Future<SpeechInput> _openInput(String directory) =>
      SpeechInput.newInstance(directory: directory);
  SpeechInput? _input;
  Future<SpeechInput>? _opening;
  SpeechInput get input =>
      _input ?? (throw StateError('speech is not initialized'));
  bool enabled = true;
  String language = 'auto';
  bool ready = false;
  bool downloading = false;
  int progress = 0;
  Object? error;
  bool _closed = false;
  bool _checking = false;
  bool _polling = false;
  bool _cancelled = false;
  int _enabledRevision = 0;
  int _languageRevision = 0;
  int _downloadRevision = 0;
  Timer? _progressTimer;

  Future<SpeechInput> _acquire() async {
    if (_closed) throw StateError('speech is closed');
    if (_input != null) return _input!;
    return _opening ??= _create();
  }

  Future<SpeechInput> _create() async {
    try {
      final opened = await _open(directory);
      if (_closed) {
        try {
          await opened.cancel();
        } finally {
          opened.dispose();
        }
        throw StateError('speech is closed');
      }
      _input = opened;
      return opened;
    } finally {
      _opening = null;
    }
  }

  Future<void> check() async {
    if (_closed || _checking) return;
    _checking = true;
    final enabledRevision = _enabledRevision;
    final languageRevision = _languageRevision;
    final downloadRevision = _downloadRevision;
    try {
      final savedEnabled = await _preferences.getBool('speech.enabled') ?? true;
      final savedLanguage =
          await _preferences.getString('speech.language') ?? 'auto';
      if (_closed) return;
      if (enabledRevision == _enabledRevision) enabled = savedEnabled;
      if (languageRevision == _languageRevision) language = savedLanguage;
      final opened = await _acquire();
      if (_closed) return;
      final available = await opened.ready();
      if (!_closed && downloadRevision == _downloadRevision) {
        ready = available;
        error = null;
      }
    } catch (failure) {
      if (!_closed && downloadRevision == _downloadRevision) error = failure;
    } finally {
      _checking = false;
      _changed();
    }
  }

  Future<void> setEnabled(bool value) async {
    if (_closed) return;
    final revision = ++_enabledRevision;
    try {
      await _preferences.setBool('speech.enabled', value);
      if (!_closed && revision == _enabledRevision) {
        enabled = value;
        error = null;
      }
    } catch (failure) {
      if (!_closed) error = failure;
    }
    _changed();
  }

  Future<void> setLanguage(String value) async {
    if (!const ['auto', 'zh', 'en'].contains(value)) {
      throw ArgumentError.value(value, 'language');
    }
    if (_closed) return;
    final revision = ++_languageRevision;
    try {
      await _preferences.setString('speech.language', value);
      if (!_closed && revision == _languageRevision) {
        language = value;
        error = null;
      }
    } catch (failure) {
      if (!_closed) error = failure;
    }
    _changed();
  }

  Future<void> download() async {
    if (downloading || _closed) return;
    _downloadRevision++;
    _cancelled = false;
    downloading = true;
    error = null;
    progress = 0;
    _changed();
    try {
      final opened = await _acquire();
      if (_closed || _cancelled) return;
      _progressTimer = Timer.periodic(
        const Duration(milliseconds: 250),
        (_) => unawaited(_progress(opened)),
      );
      await opened.download();
      if (_closed || _cancelled) return;
      final available = await opened.ready();
      if (!_closed && !_cancelled) {
        ready = available;
        error = null;
      }
    } catch (failure) {
      if (!_closed && !_cancelled) error = failure;
    } finally {
      _progressTimer?.cancel();
      _progressTimer = null;
      downloading = false;
      _changed();
    }
  }

  Future<void> _progress(SpeechInput opened) async {
    if (_closed || !downloading || _polling || _cancelled) return;
    _polling = true;
    final revision = _downloadRevision;
    try {
      final value = await opened.downloadProgress();
      if (!_closed &&
          downloading &&
          !_cancelled &&
          revision == _downloadRevision) {
        progress = value;
        _changed();
      }
    } catch (failure) {
      if (!_closed &&
          downloading &&
          !_cancelled &&
          revision == _downloadRevision) {
        error = failure;
        _changed();
      }
    } finally {
      _polling = false;
    }
  }

  Future<void> cancel() async {
    _cancelled = true;
    _progressTimer?.cancel();
    try {
      await _input?.cancel();
    } catch (failure) {
      if (!_closed) {
        error = failure;
        _changed();
      }
    }
  }

  void _changed() {
    if (!_closed) notifyListeners();
  }

  @override
  void dispose() {
    _closed = true;
    _progressTimer?.cancel();
    final opened = _input;
    _input = null;
    if (opened != null) {
      unawaited(
        (() async {
          try {
            await opened.cancel();
          } catch (_) {
            /* Disposal still releases the handle. */
          } finally {
            opened.dispose();
          }
        })(),
      );
    }
    super.dispose();
  }
}
