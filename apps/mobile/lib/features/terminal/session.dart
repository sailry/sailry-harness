import 'dart:convert';

import '../../runtime/json.dart';
import '../../runtime/session.dart';

/// Retains launch recovery while a page observes an execution-owned terminal.
class TerminalSession {
  TerminalSession({
    required this.controller,
    required this.command,
    required this.resolve,
    this.terminal,
    this.worktree,
    this.createNew = false,
  });

  final String controller;
  final CommandHandler command;
  // Asks about a retained request without sending it again.
  final Future<Map<String, dynamic>> Function(String request) resolve;
  final String? worktree;
  final bool createNew;
  String? terminal;
  String? _pending;

  Future<Map<String, dynamic>> open(
    Map<String, dynamic> launch,
    List<Map<String, dynamic>> available,
  ) async {
    if (terminal == null &&
        !createNew &&
        _pending == null &&
        worktree != null) {
      terminal =
          available
                  .where(
                    (info) =>
                        info['worktree'] == worktree &&
                        info['access'] == 'unrestricted' &&
                        info['tool'] == null &&
                        info['ssh'] == null &&
                        object(info['status'])['kind'] != 'closed',
                  )
                  .firstOrNull?['id']
              as String?;
    }
    if (terminal == null && worktree == null) {
      throw StateError('A worktree is required to create a terminal');
    }
    try {
      final response = _pending != null
          ? await resolve(_pending!)
          : terminal == null
          ? await command('create_terminal', {
              ...launch,
              'access': 'unrestricted',
              'worktree': worktree,
            })
          : await command('open_terminal', {'terminal': terminal, ...launch});
      final info = object(response['data']);
      terminal = text(info['id']);
      _pending = null;
      return info;
    } on CommandFailure catch (error) {
      _pending = error.retained ? error.request : null;
      rethrow;
    }
  }

  /// Reopening restores only this controller's lease, without claiming another's.
  int? lease(Map<String, dynamic> info) =>
      jsonEncode(info['owner']) == controller &&
          object(info['status'])['kind'] == 'running'
      ? number(info['revision']).toInt()
      : null;
}
