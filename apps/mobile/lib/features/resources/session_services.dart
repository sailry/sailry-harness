import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:sailry_bridge/api/commands.dart';

import '../../runtime/json.dart';
import '../../runtime/session.dart';
import '../../ui/kit.dart' show pushPage;
import 'port_preview.dart';

/// A session's command view from the Node subscription; empty until the first
/// update and `{'connected': false}` after the subscription fails.
class SessionServices extends ValueNotifier<Map<String, dynamic>> {
  SessionServices(this.host, this.session) : super({});

  final HostConnection host;
  final String session;
  CommandUpdates? _updates;
  bool _watching = false;
  bool _closed = false;

  Future<void> watch() async {
    if (_watching || _closed) return;
    _watching = true;
    CommandUpdates? updates;
    try {
      updates = await host.connection.watchCommands(session: session);
      if (_closed) return;
      _updates = updates;
      while (!_closed) {
        final view = object(jsonDecode(await updates.next()));
        if (_closed) return;
        value = view;
      }
    } catch (_) {
      if (!_closed) value = {'connected': false};
    } finally {
      _watching = false;
      _updates = null;
      if (updates != null) await updates.close().whenComplete(updates.dispose);
    }
  }

  /// Distinct service ports with the forwarding source that declared them.
  List<Map<String, dynamic>> sources({bool running = false}) {
    final seen = <int>{};
    return [
      for (final command in objects(value['items']))
        if (!running || object(command['status'])['kind'] == 'running')
          for (final service in objects(command['services']))
            if (service['port'] is int && seen.add(service['port'] as int))
              {
                'session': session,
                'command': command['id'],
                'service': service,
              },
    ];
  }

  @override
  void dispose() {
    _closed = true;
    final updates = _updates;
    if (updates != null) unawaited(updates.close());
    super.dispose();
  }
}

/// Forwards a session service locally and opens its preview.
Future<void> openService(
  BuildContext context,
  HostConnection host,
  Map<String, dynamic> source,
) async {
  final service = object(source['service']);
  final mapping = await host.ports.open(service['port'] as int, source: source);
  if (!context.mounted || !mapping.listening.value) return;
  final uri = Uri.parse(
    text(service['url']),
  ).replace(host: '127.0.0.1', port: mapping.local);
  await pushPage(
    context,
    PortPreviewPage(uri: uri, listening: mapping.listening),
  );
}
