import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../ui/kit.dart';
import 'tool_row.dart';
import 'turn_frame.dart';

Map<String, dynamic>? childSession(
  HostConnection host,
  Map<String, dynamic> reference,
) {
  final id = host.childTargets[reference['target']];
  if (id == null) return null;
  return objects(
    host.snapshot['sessions'],
  ).where((session) => session['id'] == id).firstOrNull;
}

bool failedRun(Map<String, dynamic> run) =>
    ['failed', 'interrupted'].contains(run['status']);

/// A sub-task call stays a step row; its session opens from the chevron.
class ChildTask extends StatelessWidget {
  const ChildTask({
    super.key,
    required this.child,
    required this.title,
    required this.connected,
    this.continuing = false,
    this.onOpen,
  });

  final Map<String, dynamic> child;
  final String title;
  final bool connected;
  final bool continuing;
  final VoidCallback? onOpen;

  @override
  Widget build(BuildContext context) {
    final run = object(child['run']);
    return ToolRow(
      key: ValueKey('child-${run['session']}'),
      icon: 'spark',
      label: context.tr('conversationChild'),
      detail: title,
      tone: failedRun(run) ? RowTone.failed : RowTone.normal,
      running: connected && (activeRun(run) || continuing),
      action: onOpen == null
          ? null
          : AppIcon(
              'chevron',
              size: 12,
              color: Theme.of(context).colorScheme.onSurfaceVariant,
            ),
      onTap: onOpen,
    );
  }
}
