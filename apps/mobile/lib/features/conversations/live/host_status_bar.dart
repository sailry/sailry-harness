import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/session.dart';
import '../../../ui/connection_status.dart';
import '../../../ui/theme.dart';
import '../../resources/host/metric_history.dart';

/// Connection line under the page title. One host shows its own state; several
/// hosts show how many are online and the fastest connected route.
class HostStatusBar extends StatelessWidget {
  const HostStatusBar({super.key, required this.hosts, required this.onTap});

  /// Hosts in scope; the caller hides the bar when there are none.
  final List<HostConnection> hosts;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final online = hosts.where((host) => host.connected).toList();
    final statuses = hosts.map(
      (host) => hostStatus(host.connected, host.error),
    );
    final status = online.isNotEmpty
        ? HostStatus.online
        : statuses.contains(HostStatus.connecting)
        ? HostStatus.connecting
        : HostStatus.offline;
    final best = online.isEmpty
        ? null
        : online.reduce(
            (a, b) =>
                (linkLatency(b.transport) ?? double.infinity) <
                    (linkLatency(a.transport) ?? double.infinity)
                ? b
                : a,
          );
    final (routeKey, latency) = connectionColumns(
      best != null,
      best?.transport ?? const {},
    );
    final route = routeKey == 'connectionRouteUnknown'
        ? null
        : context.tr(routeKey);
    final title = hosts.length == 1
        ? hosts.single.label
        : context
              .tr('hostsConnected')
              .replaceAll('{connected}', '${online.length}')
              .replaceAll('{total}', '${hosts.length}');
    final word = hosts.length == 1 || status == HostStatus.connecting
        ? context.tr(switch (status) {
            HostStatus.online => 'hostConnected',
            HostStatus.connecting => 'hostConnecting',
            HostStatus.offline => 'hostNotConnected',
          })
        : null;
    final duration = MediaQuery.disableAnimationsOf(context)
        ? Duration.zero
        : const Duration(milliseconds: 220);
    final muted = TextStyle(fontSize: 13, color: colors.onSurfaceVariant);
    return Semantics(
      button: true,
      label: [title, ?word, ?route, ?latency].join(', '),
      excludeSemantics: true,
      child: InkWell(
        key: const ValueKey('host-status-bar'),
        onTap: onTap,
        borderRadius: BorderRadius.circular(8),
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: 4),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              // The route and its latency, or the state while not online.
              // The host's name stays in the semantic label.
              AnimatedSwitcher(
                duration: duration,
                child: Row(
                  key: ValueKey((status, route, latency)),
                  mainAxisSize: MainAxisSize.min,
                  spacing: 10,
                  children: [
                    if (status != HostStatus.online && word != null)
                      Text(word, style: muted)
                    else if (route != null)
                      Text(route, style: muted),
                    if (status == HostStatus.online && latency != null)
                      Text(
                        latency,
                        style: muted.copyWith(
                          color: latencyColor(
                            context,
                            linkLatency(best!.transport),
                          ),
                          fontWeight: FontWeight.w500,
                          fontFeatures: const [FontFeature.tabularFigures()],
                        ),
                      ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Latency colors by how responsive the route feels: green up to 100 ms,
/// yellow up to 300 ms and red beyond.
Color latencyColor(BuildContext context, double? milliseconds) =>
    SailryTheme.statusColor(
      context,
      milliseconds == null || milliseconds <= 100
          ? StatusTone.success
          : milliseconds <= 300
          ? StatusTone.warning
          : StatusTone.danger,
    );
