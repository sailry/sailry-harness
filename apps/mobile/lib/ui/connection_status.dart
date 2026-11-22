import 'package:flutter/material.dart';
import 'package:sailry_bridge/bridge.dart';

import '../l10n/strings.dart';
import '../runtime/session.dart';

String? connectionFailureKey(Object? fault) {
  final mismatch = versionMismatchKey(fault);
  if (mismatch != null) return mismatch;

  final (code, message) = switch (fault) {
    BridgeFailure(:final code, :final message) => (code, message),
    CommandFailure(:final code, :final detail) => (code, detail),
    Map() => (fault['code'], fault['message']),
    _ => (null, null),
  };

  if (message is String) {
    final diagnostic = message.trim().toLowerCase();
    if (code == 'permission_denied' &&
        const {
          'peer identity could not be verified',
          'libp2p peer identity could not be verified',
          'libp2p stream peer identity could not be verified',
        }.contains(diagnostic)) {
      return 'connectionFailureAuthentication';
    }
    if (code == 'unavailable') {
      final unreachable = _networkFailure.hasMatch(diagnostic);
      final timedOut = _hostTimeout.hasMatch(diagnostic);
      if (unreachable && !timedOut) return 'connectionFailureNetwork';
      if (timedOut && !unreachable) return 'connectionFailureHost';
    }
  }
  return switch (code) {
    'permission_denied' => 'failureDenied',
    'busy' => 'failureBusy',
    _ => null,
  };
}

final _networkFailure = RegExp(
  r'\b(network is unreachable|network is down|no route to host)\b',
);
final _hostTimeout = RegExp(
  r'\b(context deadline exceeded|i/o timeout|connection timed out)\b',
);

/// Selected transport and its RTT are separate columns, never inferred from reachability.
class ConnectionStatus extends StatelessWidget {
  const ConnectionStatus({
    super.key,
    required this.connected,
    required this.transport,
  });
  final bool connected;
  final Map<String, dynamic> transport;

  @override
  Widget build(BuildContext context) {
    final (key, latency) = connectionColumns(connected, transport);
    return DefaultTextStyle.merge(
      style: TextStyle(color: Theme.of(context).colorScheme.onSurfaceVariant),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          SizedBox(
            width: 56,
            child: Text(context.tr(key), textAlign: TextAlign.right),
          ),
          const SizedBox(width: 12),
          SizedBox(
            width: 80,
            child: Text(
              latency ?? context.tr('connectionRouteUnknown'),
              textAlign: TextAlign.right,
            ),
          ),
        ],
      ),
    );
  }
}

(String, String?) connectionColumns(
  bool connected,
  Map<String, dynamic> transport,
) {
  final route = connected ? transport['route'] : null;
  final key = switch (route) {
    'p2p' => 'connectionRouteP2p',
    'relay' => 'connectionRouteRelay',
    _ => 'connectionRouteUnknown',
  };
  final rtt = connected && (route == 'p2p' || route == 'relay')
      ? transport['rtt_ms']
      : null;
  return (
    key,
    rtt is num && rtt.isFinite && rtt >= 0 ? '${rtt.round()} ms' : null,
  );
}
