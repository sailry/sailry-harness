import 'package:flutter/foundation.dart';

import '../../../runtime/json.dart';
import 'tool_display.dart' show OutputLoader;

/// Retains only the lazily requested presentation for the current tool result.
class DeferredOutput extends ChangeNotifier {
  Object? _identity;
  Object? _loading;
  Object? _failed;
  Map<String, dynamic>? _value;
  bool _disposed = false;

  Map<String, dynamic>? get value => _value;
  bool get failed => _failed == _identity && _identity != null;

  void bind(Map<String, dynamic> call) {
    final source = object(call['source']);
    final response = object(call['response']);
    final identity = (
      source['entry'],
      source['index'],
      response['entry'],
      response['index'],
      call['revision'],
    );
    if (_identity == identity) return;
    _identity = identity;
    _loading = null;
    _failed = null;
    _value = null;
  }

  Future<void> resolve(
    Map<String, dynamic> call, {
    required bool expanded,
    required bool enabled,
    OutputLoader? load,
    bool publication = false,
    bool retry = false,
  }) async {
    bind(call);
    final approval = object(call['approval'])['state'] == 'pending';
    if (_disposed ||
        (!expanded && !approval) ||
        !enabled ||
        (!approval &&
            (publication
                ? call['published'] != true
                : call['details'] != true)) ||
        (failed && !retry) ||
        _value != null ||
        _loading != null ||
        load == null) {
      return;
    }
    final identity = _identity;
    _failed = null;
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
      if (current()) {
        _value = output;
        if (output == null) _failed = identity;
        notifyListeners();
      }
    } catch (_) {
      if (current()) {
        _failed = identity;
        notifyListeners();
        rethrow;
      }
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
