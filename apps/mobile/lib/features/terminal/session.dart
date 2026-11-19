import 'dart:convert';

import '../../runtime/json.dart';
import '../../runtime/session.dart';

/// Retains launch recovery while a page observes an execution-owned terminal.
class TerminalSession {
  TerminalSession({
    required this.controller,
    required this.command,
    required this.resolve,
    this.sharedEntry,
    this.terminal,
    this.worktree,
    this.createNew = false,
  });

  final String? controller;
  final String? sharedEntry;
  final CommandHandler command;
  // Asks about a retained request without sending it again.
  final Future<Map<String, dynamic>> Function(String request) resolve;
  final String? worktree;
  final bool createNew;
  String? terminal;
  String? _pending;
  String? _pendingClaim;
  ({String terminal, int revision})? _lease;

  int? get revision => _lease?.revision;

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
                        object(info['status'])['kind'] == 'running',
                  )
                  .firstOrNull?['id']
              as String?;
    }
    if (terminal == null && worktree == null) {
      throw StateError('A worktree is required to create a terminal');
    }
    try {
      final creating = terminal == null;
      final response = _pending != null
          ? await resolve(_pending!)
          : creating
          ? await command('create_terminal', {
              ...launch,
              'access': 'unrestricted',
              'worktree': worktree,
            })
          : await command('open_terminal', {'terminal': terminal, ...launch});
      final info = object(response['data']);
      terminal = text(info['id']);
      if (creating || sharedEntry == null) {
        _lease = _ownedLease(info);
      } else if (lease(info) == null) {
        forgetLease();
      }
      _pending = null;
      return info;
    } on CommandFailure catch (error) {
      _pending = error.retained ? error.request : null;
      rethrow;
    }
  }

  Future<void> claim(int expectedRevision) async {
    try {
      final result = _pendingClaim != null
          ? await resolve(_pendingClaim!)
          : await command('claim_terminal', {
              'terminal': terminal,
              'expected_revision': expectedRevision,
            });
      _pendingClaim = null;
      _lease = _ownedLease(object(result['data']));
    } on CommandFailure catch (error) {
      if (error.retained) _pendingClaim = error.request;
      if (error.rejected) _pendingClaim = null;
      rethrow;
    }
  }

  void forgetLease() => _lease = null;

  void observe(Map<String, dynamic> info) {
    final active = _lease;
    if (active != null &&
        number(info['revision']) >= active.revision &&
        lease(info) == null) {
      forgetLease();
    }
  }

  int? lease(Map<String, dynamic> info) {
    final active = _lease;
    return active != null && active == _ownedLease(info)
        ? active.revision
        : null;
  }

  ({String terminal, int revision})? _ownedLease(Map<String, dynamic> info) {
    final id = info['id'];
    final revision = info['revision'];
    if (id is! String ||
        id.isEmpty ||
        id != terminal ||
        revision is! int ||
        revision < 1 ||
        object(info['status'])['kind'] != 'running') {
      return null;
    }
    // The entry is shared with other routed controllers, so matching it only
    // confirms a lease after this page has created or claimed the terminal.
    final owner = info['owner'];
    final ours = sharedEntry == null
        ? jsonEncode(owner) == controller
        : nodeIdHex(owner) == sharedEntry;
    return ours ? (terminal: id, revision: revision) : null;
  }
}
