import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/session.dart';
import '../../../ui/connection_status.dart';
import '../../resources/host/host_info_card.dart';
import '../../resources/host/metric_history.dart';
import '../task_row.dart';

/// Connection row below the project bar. One host shows its own state; several
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
      child: Material(
        color: colors.surfaceContainer,
        shape: const StadiumBorder(),
        clipBehavior: Clip.antiAlias,
        child: InkWell(
          key: const ValueKey('host-status-bar'),
          onTap: onTap,
          child: SizedBox(
            height: 34,
            // The dot sits under the project badge and the title under the
            // project name, so both bars read as one column.
            child: Padding(
              padding: const EdgeInsets.fromLTRB(17, 0, 14, 0),
              child: Row(
                children: [
                  TweenAnimationBuilder<Color?>(
                    tween: ColorTween(end: statusDotColor(context, status)),
                    duration: duration,
                    builder: (context, color, _) => PulseDot(
                      color: color!,
                      pulse: status == HostStatus.connecting,
                    ),
                  ),
                  const SizedBox(width: 18),
                  Expanded(
                    child: Row(
                      children: [
                        Flexible(
                          child: Text(
                            title,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: TextStyle(
                              fontSize: 13,
                              color: colors.onSurface,
                            ),
                          ),
                        ),
                        if (word != null) ...[
                          const SizedBox(width: 8),
                          AnimatedSwitcher(
                            duration: duration,
                            child: Text(
                              word,
                              key: ValueKey(word),
                              style: muted,
                            ),
                          ),
                        ],
                      ],
                    ),
                  ),
                  const SizedBox(width: 8),
                  AnimatedSwitcher(
                    duration: duration,
                    child: Row(
                      key: ValueKey((route, latency)),
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        if (route != null)
                          Container(
                            padding: const EdgeInsets.symmetric(
                              horizontal: 7,
                              vertical: 2,
                            ),
                            decoration: BoxDecoration(
                              color: colors.surface,
                              borderRadius: BorderRadius.circular(6),
                            ),
                            child: Text(
                              route,
                              style: TextStyle(
                                fontSize: 11,
                                fontWeight: FontWeight.w600,
                                color: colors.onSurfaceVariant,
                              ),
                            ),
                          ),
                        if (latency != null) ...[
                          const SizedBox(width: 8),
                          Text(
                            latency,
                            style: muted.copyWith(
                              fontFeatures: const [
                                FontFeature.tabularFigures(),
                              ],
                            ),
                          ),
                        ],
                      ],
                    ),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}
