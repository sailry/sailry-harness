import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import 'team.dart';

/// Keeps a configuration operation's continuation with its uncertain request.
/// Recovery uses the original request and advances only after confirmed success.
class ConfigurationCommands {
  ConfigurationCommands({
    required this.command,
    required this.execute,
    required this.accept,
    required this.active,
  });

  final CommandHandler command;
  final Future<Map<String, dynamic>> Function(String request) execute;
  final void Function(Map<String, dynamic> response) accept;
  final bool Function() active;
  ({String request, Future<void> Function()? after})? _pending;

  bool get pending => _pending != null;

  Future<void> select(
    Map<String, dynamic>? team, {
    required Map<String, dynamic> Function() session,
    required List<Map<String, dynamic>> Function() providers,
  }) {
    final current = session();
    return send(
      'set_session_team',
      selectCommand(current['id'] as String, current['revision'] as int, team),
      after: team == null
          ? null
          : () async {
              final current = session();
              final config = leadConfig(
                object(current['config']),
                team,
                providers(),
              );
              if (config == null) return;
              await send('set_session_config', {
                'session': current['id'],
                'expected_revision': current['revision'],
                'config': config,
              });
            },
    );
  }

  Future<void> send(
    String kind,
    Map<String, dynamic> data, {
    Future<void> Function()? after,
  }) async {
    if (pending || !active()) return;
    Map<String, dynamic> response;
    try {
      response = await command(kind, data);
    } on CommandFailure catch (error) {
      final request = error.request;
      if (error.code == 'outcome_unknown' && request != null) {
        _pending = (request: request, after: after);
      }
      rethrow;
    }
    if (!active()) return;
    accept(response);
    if (active()) await after?.call();
  }

  Future<void> check() async {
    final pending = _pending;
    if (pending == null || !active()) return;
    Map<String, dynamic> response;
    try {
      response = await execute(pending.request);
    } on CommandFailure catch (error) {
      if (error.rejected) _pending = null;
      rethrow;
    }
    // Clear this request before its continuation can submit another command.
    _pending = null;
    if (!active()) return;
    accept(response);
    if (active()) await pending.after?.call();
  }
}
