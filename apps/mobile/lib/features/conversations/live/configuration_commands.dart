import '../../../runtime/session.dart';
import 'team.dart';

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
  final void Function(Map<String, dynamic> response) accept;
  final bool Function() active;
  String? _pending;

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
      if (error.retained) _pending = error.request;
      rethrow;
    }
    if (!active()) return;
    accept(response);
  }

  Future<void> check() async {
    final pending = _pending;
    if (pending == null || !active()) return;
    Map<String, dynamic> response;
    try {
      response = await resolve(pending);
    } on CommandFailure catch (error) {
      if (error.rejected) _pending = null;
      rethrow;
    }
    _pending = null;
    if (!active()) return;
    accept(response);
  }
}
