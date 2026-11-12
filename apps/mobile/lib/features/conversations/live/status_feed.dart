import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:sailry_bridge/api/status.dart';

import '../../../runtime/json.dart';
import '../../../runtime/session.dart';

/// Observes Node state; only request retries and subscription lifetime are local.
class StatusFeed extends ValueNotifier<Map<String, dynamic>> {
  StatusFeed(this.host, Map<String, dynamic> session)
    : scope = _scope(session),
      super({});

  final HostConnection host;
  final String scope;
  StatusUpdates? _updates;
  bool _watching = false;
  bool _closed = false;
  final _running = <String, Object>{};
  final _requests = <String, ({String request, Map<String, dynamic> entry})>{};

  static String _scope(Map<String, dynamic> session) =>
      jsonEncode({'worktree': session['worktree'], 'session': session['id']});

  /// A moved conversation owns a fresh observer and no requests from its old scope.
  StatusFeed rebind(HostConnection host, Map<String, dynamic> session) {
    if (!_closed && identical(this.host, host) && scope == _scope(session)) {
      return this;
    }
    if (!_closed) dispose();
    return StatusFeed(host, session);
  }

  @override
  set value(Map<String, dynamic> view) {
    final captures = objects(view['entries']).map(_key).toSet();
    _requests.removeWhere((key, _) => !captures.contains(key));
    // Retiring a capture also invalidates its eventual command completion.
    _running.removeWhere((key, _) => !captures.contains(key));
    super.value = view;
  }

  bool get connected => host.connected && value['connected'] == true;
  List<Map<String, dynamic>> get entries {
    final entries = <Map<String, dynamic>>[];
    for (final entry in objects(value['entries'])) {
      final key = _key(entry);
      if (object(entry['state'])['visible'] == true ||
          _requests.containsKey(key)) {
        entries.add(_requests[key]?.entry ?? entry);
      }
    }
    return entries;
  }

  String _key(Map<String, dynamic> entry) {
    final package = object(entry['package']);
    return jsonEncode([
      package['name'],
      package['digest'],
      package['settings_revision'],
      object(entry['declaration'])['id'],
    ]);
  }

  Map<String, dynamic>? current(Map<String, dynamic> entry) =>
      _requests[_key(entry)]?.entry ??
      objects(
        value['entries'],
      ).where((candidate) => _key(candidate) == _key(entry)).firstOrNull;

  bool retry(Map<String, dynamic> entry) => _requests.containsKey(_key(entry));

  bool enabled(Map<String, dynamic> entry) {
    if (_closed || !connected || _running.containsKey(_key(entry))) {
      return false;
    }
    if (retry(entry)) return true;
    final selected = current(entry);
    final state = object(selected?['state']);
    return state['visible'] == true && state['enabled'] == true;
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
    final selected = current(entry)!;
    var request = _requests[key]?.request;
    final operation = Object();
    _running[key] = operation;
    notifyListeners();
    try {
      request ??= await host.connection.prepareStatus(
        scope: scope,
        entry: jsonEncode(selected),
      );
      await host.execute(request);
      if (identical(_running[key], operation)) _requests.remove(key);
    } catch (failure) {
      if (!_closed && identical(_running[key], operation)) {
        if (request != null && failure is CommandFailure && failure.retained) {
          _requests[key] = (request: request, entry: selected);
        } else {
          _requests.remove(key);
        }
      }
      rethrow;
    } finally {
      if (identical(_running[key], operation)) _running.remove(key);
      if (!_closed) notifyListeners();
    }
  }

  @override
  void dispose() {
    _closed = true;
    _requests.clear();
    _running.clear();
    final updates = _updates;
    if (updates != null) unawaited(updates.close());
    super.dispose();
  }
}
