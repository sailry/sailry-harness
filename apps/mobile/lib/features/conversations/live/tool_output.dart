import 'package:flutter/foundation.dart';

import '../../../runtime/json.dart';
import 'tool_display.dart' show OutputLoader;

/// Retains only the lazily requested presentation for the current tool result.
class DeferredOutput extends ChangeNotifier {
  Object? _identity;
  Object? _loading;
  Map<String, dynamic>? _value;
  bool _disposed = false;

  Map<String, dynamic>? get value => _value;

  void bind(Map<String, dynamic> call) {
    final source = object(call['source']);
    final response = object(call['response']);
    final identity = (
      source['entry'],
      source['index'],
      response['entry'],
      response['index'],
    );
    if (_identity == identity) return;
    _identity = identity;
    _loading = null;
    _value = null;
  }

  Future<void> resolve(
    Map<String, dynamic> call, {
    required bool expanded,
    required bool enabled,
    OutputLoader? load,
  }) async {
    bind(call);
    if (_disposed ||
        !expanded ||
        !enabled ||
        call['details'] != true ||
        call['output_deferred'] != true ||
        _value != null ||
        _loading != null ||
        load == null) {
      return;
    }
    final identity = _identity;
    final request = Object();
    final source = object(call['source']);
    _loading = request;
    bool current() =>
        !_disposed && _identity == identity && identical(_loading, request);
    try {
      final output = await load(
        source['entry'] as String,
        source['index'] as int,
      );
      if (current() && output != null) {
        _value = output;
        notifyListeners();
      }
    } catch (_) {
      if (current()) rethrow;
    } finally {
      if (current()) _loading = null;
    }
  }

  @override
  void dispose() {
    _disposed = true;
    super.dispose();
  }
}
