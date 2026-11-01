import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/session.dart';
import '../../../ui/kit.dart';
import '../../../ui/theme.dart';
import '../../../ui/toast.dart';
import '../../conversations/live/project_bar.dart';
import 'host_info_card.dart';
import 'host_parts.dart';
import 'metric_history.dart';

Color _hostTone(BuildContext context, HostConnection host) {
  if (!host.connected) return Theme.of(context).colorScheme.onSurfaceVariant;
  return linkRoute(host.transport) == 'relay'
      ? SailryTheme.statusColor(context, StatusTone.warning)
      : SailryTheme.accentColor(context, AccentTone.blue);
}

/// Project-bar styled switcher for the selected host.
class HostBar extends StatelessWidget {
  const HostBar({super.key, required this.host, required this.onTap});
  final HostConnection host;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final system = systemLabel(host.info);
    return ProjectBar(
      project: null,
      title: host.label,
      detail: system.isEmpty ? null : system,
      leading: AppIcon('server', size: 15, color: _hostTone(context, host)),
      status: StatusDot(
        color: statusDotColor(context, hostStatus(host.connected, host.error)),
      ),
      onTap: onTap,
    );
  }
}

/// Lists hosts with their route, latency and local pairing actions.
Future<void> showHostSheet(
  BuildContext context, {
  required ValueChanged<String>? onSettings,
  required VoidCallback onPair,
}) {
  final session = AppSession.of(context);
  return showAppSheet<void>(
    context,
    context.tr('selectHost'),
    child: _HostList(session: session, onSettings: onSettings, onPair: onPair),
  );
}

class _HostList extends StatefulWidget {
  const _HostList({
    required this.session,
    required this.onSettings,
    required this.onPair,
  });
  final AppSession session;
  final ValueChanged<String>? onSettings;
  final VoidCallback onPair;

  @override
  State<_HostList> createState() => _HostListState();
}

class _HostListState extends State<_HostList> {
  final _removing = <String>{};

  Future<void> _remove(HostConnection target) async {
    if (_removing.contains(target.id)) return;
    final session = widget.session;
    setState(() => _removing.add(target.id));
    try {
      final confirmed = await showAppSheet<bool>(
        context,
        context.tr('hostRemove'),
        child: Builder(
          builder: (context) => Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(
                context
                    .tr('hostRemoveConfirm')
                    .replaceAll('{name}', target.label),
              ),
              const SizedBox(height: 20),
              Row(
                children: [
                  Expanded(
                    child: OutlinedButton(
                      onPressed: () => Navigator.pop(context, false),
                      child: Text(context.tr('cancel')),
                    ),
                  ),
                  const SizedBox(width: 12),
                  Expanded(
                    child: FilledButton(
                      onPressed: () => Navigator.pop(context, true),
                      child: Text(context.tr('hostRemove')),
                    ),
                  ),
                ],
              ),
            ],
          ),
        ),
      );
      if (confirmed != true || !mounted) return;
      await session.removeHost(target);
      if (mounted) showToast(context, context.tr('hostRemoved'));
    } catch (_) {
      if (mounted) showToast(context, context.tr('hostRemoveFailed'));
    } finally {
      if (mounted) setState(() => _removing.remove(target.id));
    }
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: widget.session,
    builder: (context, _) => Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        for (final (index, host) in widget.session.hosts.indexed) ...[
          if (index > 0) const Divider(),
          _HostRow(
            host: host,
            onTap: () {
              Navigator.pop(context);
              widget.session.selectHost(host.id);
            },
            onSettings: widget.onSettings == null
                ? null
                : () {
                    Navigator.pop(context);
                    widget.onSettings!(host.id);
                  },
            onRemove: _removing.contains(host.id) ? null : () => _remove(host),
          ),
        ],
        const SizedBox(height: 14),
        TextButton(
          style: TextButton.styleFrom(
            minimumSize: const Size.fromHeight(42),
            backgroundColor: Theme.of(context).colorScheme.surfaceContainerHigh,
            shape: const StadiumBorder(),
          ),
          onPressed: () {
            Navigator.pop(context);
            widget.onPair();
          },
          child: Text(context.tr('pair')),
        ),
      ],
    ),
  );
}

class _HostRow extends StatelessWidget {
  const _HostRow({
    required this.host,
    required this.onTap,
    required this.onSettings,
    required this.onRemove,
  });
  final HostConnection host;
  final VoidCallback onTap;
  final VoidCallback? onSettings;
  final VoidCallback? onRemove;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final status = hostStatus(host.connected, host.error);
    final route = linkRoute(host.transport);
    final latency = linkLatency(host.transport);
    final detail = switch (status) {
      HostStatus.online => [
        if (route != null)
          context.tr(
            route == 'p2p' ? 'connectionRouteP2p' : 'connectionRouteRelay',
          ),
        if (latency != null) '${latency.round()} ms',
      ].join('  '),
      HostStatus.connecting => context.tr('hostConnecting'),
      HostStatus.offline => context.tr('offline'),
    };
    return InkWell(
      key: ValueKey('host-row-${host.id}'),
      borderRadius: BorderRadius.circular(12),
      onTap: onTap,
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 8),
        child: Row(
          children: [
            Container(
              width: 34,
              height: 34,
              decoration: BoxDecoration(
                color: colors.surfaceContainerHigh,
                shape: BoxShape.circle,
              ),
              alignment: Alignment.center,
              child: AppIcon(
                'server',
                size: 17,
                color: _hostTone(context, host),
              ),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      Flexible(
                        child: Text(
                          host.label,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                            fontSize: 14,
                            color: host.connected
                                ? colors.onSurface
                                : colors.onSurfaceVariant,
                          ),
                        ),
                      ),
                      const SizedBox(width: 7),
                      StatusDot(color: statusDotColor(context, status)),
                    ],
                  ),
                  if (detail.isNotEmpty) ...[
                    const SizedBox(height: 2),
                    Text(
                      detail,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: dimStyle(context),
                    ),
                  ],
                ],
              ),
            ),
            if (onSettings != null)
              IconButton(
                tooltip: context.tr('settings'),
                onPressed: onSettings,
                icon: const AppIcon('settings', size: 18),
              ),
            IconButton(
              tooltip: context.tr('hostRemove'),
              onPressed: onRemove,
              icon: const AppIcon('trash', size: 18),
            ),
          ],
        ),
      ),
    );
  }
}
