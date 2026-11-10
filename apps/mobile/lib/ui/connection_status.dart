import 'package:flutter/material.dart';
import '../l10n/strings.dart';

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
