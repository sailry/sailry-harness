import '../../../runtime/session.dart';
import 'team.dart';

/// Keeps only the local changes when a rejected save reads a newer revision.
Map<String, dynamic> rebaseConfiguration(
  Map<String, dynamic> baseline,
  Map<String, dynamic> draft,
  Map<String, dynamic> current,
) {
  final result = Map<String, dynamic>.of(current);
  for (final key in {...baseline.keys, ...draft.keys}) {
    if (baseline.containsKey(key) == draft.containsKey(key) &&
        _sameValue(baseline[key], draft[key])) {
      continue;
    }
    if (!draft.containsKey(key)) {
      result.remove(key);
    } else if (baseline[key] is Map && draft[key] is Map) {
      result[key] = rebaseConfiguration(
        Map<String, dynamic>.from(baseline[key] as Map),
        Map<String, dynamic>.from(draft[key] as Map),
        current[key] is Map
            ? Map<String, dynamic>.from(current[key] as Map)
            : {},
      );
    } else {
      result[key] = draft[key];
    }
  }
  return result;
}

bool _sameValue(Object? a, Object? b) {
  if (a is Map && b is Map) {
    return a.length == b.length &&
        a.keys.every((key) => b.containsKey(key) && _sameValue(a[key], b[key]));
  }
  if (a is List && b is List) {
    return a.length == b.length &&
        Iterable<int>.generate(a.length).every((i) => _sameValue(a[i], b[i]));
  }
  return a == b;
}

/// Recovers an uncertain configuration operation using its original request.
class ConfigurationCommands {
  ConfigurationCommands({
    required this.command,
    required this.resolve,
    required this.accept,
    required this.active,
  });

  final CommandHandler command;
  // Asks about a retained request without sending it again.
  final Future<Map<String, dynamic>> Function(String request) resolve;
  final void Function(Map<String, dynamic> response, String kind) accept;
  final bool Function() active;
  String? _pending;
  String? _pendingKind;

  bool get pending => _pending != null;

  Future<void> select(
    Map<String, dynamic>? team, {
    required Map<String, dynamic> Function() session,
  }) {
    final current = session();
    return send(
      'set_session_team',
      selectCommand(current['id'] as String, current['revision'] as int, team),
    );
  }

  Future<void> send(String kind, Map<String, dynamic> data) async {
    if (pending || !active()) return;
    Map<String, dynamic> response;
    try {
      response = await command(kind, data);
    } on CommandFailure catch (error) {
      if (error.retained) {
        _pending = error.request;
        _pendingKind = kind;
      }
      rethrow;
    }
    if (!active()) return;
    accept(response, kind);
  }

  Future<void> check() async {
    final pending = _pending;
    if (pending == null || !active()) return;
    Map<String, dynamic> response;
    try {
      response = await resolve(pending);
    } on CommandFailure catch (error) {
      if (error.rejected) {
        _pending = null;
        _pendingKind = null;
      }
      rethrow;
    }
    final kind = _pendingKind!;
    _pending = null;
    _pendingKind = null;
    if (!active()) return;
    accept(response, kind);
  }
}
