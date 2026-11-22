import 'package:flutter/widgets.dart';

import '../../l10n/strings.dart';
import '../../ui/kit.dart';
import 'contributions.dart';
import 'overview_cards.dart';
import 'ports_page.dart';

const portsResource = ProjectResourceFeature(
  entry: _portsEntry,
  entryChanges: _portChanges,
);

Listenable _portChanges(ProjectResourceView view) => view.host.ports;

ResourceEntry _portsEntry(BuildContext context, ProjectResourceView view) {
  final count = view.host.ports.entries.length;
  return (
    icon: 'globe',
    label: context.tr('activityPorts'),
    detail: count == 0 ? null : '$count',
    onTap: () => pushPage(context, PortsPage(host: view.host)),
  );
}
