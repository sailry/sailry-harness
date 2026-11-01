import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:sailry_bridge/api/status.dart';

import '../../../runtime/json.dart';
import '../../../runtime/session.dart';

/// Observes Node state; only request retries and subscription lifetime are local.
class StatusFeed extends ValueNotifier<Map<String, dynamic>> {
  StatusFeed(this.host, Map<String, dynamic> session)
    : scope = jsonEncode({
        'worktree': session['worktree'],
        'session': session['id'],
      }),
      super({});

  final HostConnection host;
  final String scope;
  StatusUpdates? _updates;
  bool _watching = false;
  bool _closed = false;
  final _running = <String>{};
  final _requests = <String, String>{};

  bool get connected => host.connected && value['connected'] == true;
  List<Map<String, dynamic>> get entries => objects(
    value['entries'],
  ).where((entry) => object(entry['state'])['visible'] == true).toList();

  String _key(Map<String, dynamic> entry) {
    final package = object(entry['package']);
    return jsonEncode([
      package['name'],
      package['digest'],
      package['settings_revision'],
      object(entry['declaration'])['id'],
    ]);
  }

  Map<String, dynamic>? current(Map<String, dynamic> entry) => objects(
    value['entries'],
  ).where((candidate) => _key(candidate) == _key(entry)).firstOrNull;

  bool enabled(Map<String, dynamic> entry) {
    final selected = current(entry);
    final state = object(selected?['state']);
    return !_closed &&
        connected &&
        state['visible'] == true &&
        state['enabled'] == true &&
        !_running.contains(_key(entry));
  }

  bool invokable(Map<String, dynamic> entry) =>
      object(entry['declaration'])['kind'] == 'button' &&
      entry['invoke'] is String;

  Future<void> watch() async {
    if (_watching || _closed) return;
    _watching = true;
    StatusUpdates? updates;
    try {
      updates = await host.connection.watchStatus(scope: scope);
      if (_closed) return;
      _updates = updates;
      while (!_closed) {
        final view = object(jsonDecode(await updates.next()));
        if (_closed) return;
        value = view;
      }
    } catch (_) {
      if (!_closed) value = {...value, 'connected': false};
    } finally {
      _watching = false;
      _updates = null;
      if (updates != null) await updates.close().whenComplete(updates.dispose);
    }
  }

  Future<void> invoke(Map<String, dynamic> entry) async {
    if (!enabled(entry) || !invokable(entry)) return;
    final key = _key(entry);
    _running.add(key);
    notifyListeners();
    try {
      final request = _requests[key] ??= await host.connection.prepareStatus(
        scope: scope,
        entry: jsonEncode(current(entry)),
      );
      await host.execute(request);
      _requests.remove(key);
    } catch (failure) {
      if (failure is! CommandFailure || failure.code != 'outcome_unknown') {
        _requests.remove(key);
      }
      rethrow;
    } finally {
      _running.remove(key);
      if (!_closed) notifyListeners();
    }
  }

  @override
  void dispose() {
    _closed = true;
    final updates = _updates;
    if (updates != null) unawaited(updates.close());
    super.dispose();
  }
}
