import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_bridge/api/ports.dart';

import 'json.dart';

/// Presentation handles for this phone's mappings; Rust owns their lifetime.
class PortMappings extends ChangeNotifier {
  PortMappings(this._connection);
  final Connection Function() _connection;
  final entries = <int, PortMapping>{};
  final _pending = <int, Future<PortMapping>>{};
  bool _closed = false;
  bool get busy => _pending.isNotEmpty;

  Future<PortMapping> open(int port, {Map<String, dynamic>? source}) {
    if (_closed) return Future.error(StateError('connection is closed'));
    final existing = entries[port];
    if (existing != null && existing.listening.value) {
      return Future.value(existing);
    }
    final pending = _pending[port];
    if (pending != null) return pending;
    final operation = _create(port, source).whenComplete(() {
      _pending.remove(port);
      if (!_closed) notifyListeners();
    });
    _pending[port] = operation;
    notifyListeners();
    return operation;
  }

  Future<PortMapping> _create(int port, Map<String, dynamic>? source) async {
    await remove(port);
    Forwarding? handle;
    try {
      if (_closed) throw StateError('connection is closed');
      handle = source == null
          ? await _connection().forwardPort(remotePort: port, localPort: 0)
          : await _connection().forwardService(
              source: jsonEncode(source),
              localPort: 0,
            );
      final local = await handle.localPort();
      if (_closed) throw StateError('connection is closed');
      final url = source == null
          ? Uri(scheme: 'http', host: '127.0.0.1', port: local)
          : Uri.parse(
              text(object(source['service'])['url']),
            ).replace(host: '127.0.0.1', port: local);
      final entry = PortMapping(port, local, url, handle);
      entries[port] = entry;
      handle = null;
      notifyListeners();
      unawaited(_watch(entry));
      return entry;
    } finally {
      if (handle != null) await handle.close().whenComplete(handle.dispose);
    }
  }

  Future<void> _watch(PortMapping entry) async {
    try {
      while (!_closed && identical(entries[entry.remote], entry)) {
        final state = object(jsonDecode(await entry.handle.next()));
        if (_closed || !identical(entries[entry.remote], entry)) return;
        entry.listening.value = state['kind'] == 'listening';
        entry.error = state['kind'] == 'failed'
            ? text(object(state['error'])['message'])
            : null;
        notifyListeners();
        if (!entry.listening.value) return;
      }
    } catch (error) {
      if (!_closed && identical(entries[entry.remote], entry)) {
        entry.listening.value = false;
        entry.error = error;
        notifyListeners();
      }
    }
  }

  Future<void> remove(int port) async {
    final entry = entries.remove(port);
    if (entry == null) return;
    entry.listening.value = false;
    if (!_closed) notifyListeners();
    await entry.handle.close().whenComplete(entry.handle.dispose);
  }

  Future<void> close() async {
    if (_closed) return;
    _closed = true;
    final pending = _pending.values.toList();
    await Future.wait<void>([
      for (final port in entries.keys.toList()) remove(port),
      for (final operation in pending)
        operation.then<void>(
          (_) {},
          onError: (Object error, StackTrace stack) {
            // The initiating view owns failures from an in-flight open.
          },
        ),
    ]);
  }
}

class PortMapping {
  PortMapping(this.remote, this.local, this.uri, this.handle);
  final int remote;
  final int local;
  final Uri uri;
  final Forwarding handle;
  final listening = ValueNotifier(true);
  Object? error;
}
