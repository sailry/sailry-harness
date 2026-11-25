import 'package:flutter/widgets.dart';

import '../../l10n/strings.dart';
import '../../runtime/session.dart';
import '../../ui/kit.dart';
import 'contributions.dart';
import 'overview_cards.dart';
import 'ports_page.dart';

const portsResource = ProjectResourceFeature(
  entry: _portsEntry,
  entryChanges: _portChanges,
  hostCount: HostResourceCount(
    id: 'ports',
    label: 'activityPorts',
    count: _portCount,
    changes: _hostPorts,
    open: _openHostPorts,
  ),
);

Listenable _portChanges(ProjectResourceView view) => view.host.ports;

Listenable _hostPorts(HostConnection host) => host.ports;

int _portCount(HostConnection host) => host.ports.entries.length;

VoidCallback _openHostPorts(BuildContext context, HostResourceView view) =>
    () => pushPage(context, PortsPage(host: view.host));

ResourceEntry _portsEntry(BuildContext context, ProjectResourceView view) {
  final count = _portCount(view.host);
  return (
    icon: 'globe',
    label: context.tr('activityPorts'),
    detail: count == 0 ? null : '$count',
    onTap: () => pushPage(context, PortsPage(host: view.host)),
  );
}
