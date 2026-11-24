import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/session.dart';
import '../../../ui/kit.dart';
import '../../../ui/toast.dart';
import '../../conversations/live/project_bar.dart';
import '../scope_picker.dart';
import 'host_info_card.dart';
import 'metric_history.dart';

export '../scope_picker.dart' show HostRow;

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
      leading: AppIcon('server', size: 15, color: hostTone(context, host)),
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
  required VoidCallback onPair,
}) {
  final session = AppSession.of(context);
  return showAppSheet<void>(
    context,
    context.tr('selectHost'),
    child: _HostList(session: session, onPair: onPair),
  );
}

class _HostList extends StatefulWidget {
  const _HostList({required this.session, required this.onPair});
  final AppSession session;
  final VoidCallback onPair;

  @override
  State<_HostList> createState() => _HostListState();
}

class _HostListState extends State<_HostList> {
  final _removing = <String>{};

  bool _busy(HostConnection target) =>
      _removing.contains(target.id) || widget.session.isRemovingHost(target);

  Future<void> _remove(HostConnection target) async {
    if (_busy(target)) return;
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
        ScopePicker(
          rows: [
            for (final host in widget.session.hosts)
              HostRow(
                host: host,
                selected: widget.session.selectedHost?.id == host.id,
                onTap: _busy(host)
                    ? null
                    : () {
                        Navigator.pop(context);
                        widget.session.selectHost(host.id);
                      },
                trailing: host.routed
                    ? null
                    : IconButton(
                        key: ValueKey('remove-host-${host.id}'),
                        tooltip: context.tr('hostRemove'),
                        onPressed: _busy(host) ? null : () => _remove(host),
                        icon: _busy(host)
                            ? SizedBox.square(
                                dimension: 18,
                                child: CircularProgressIndicator.adaptive(
                                  strokeWidth: 2,
                                  semanticsLabel: context.tr('loading'),
                                ),
                              )
                            : const AppIcon('trash', size: 18),
                      ),
              ),
          ],
        ),
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
