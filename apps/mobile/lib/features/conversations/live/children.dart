import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../ui/identicon.dart';
import '../../../ui/kit.dart';
import 'background_data.dart';
import 'connection_status.dart';
import 'presentation.dart';
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
    final running = connected && (activeRun(run) || continuing);
    final status = switch (run['status']) {
      'queued' => 'conversationQueued',
      'running' => 'running',
      'stopping' => 'conversationStopping',
      'continued' => 'conversationContinued',
      'completed' ||
      'failed' ||
      'interrupted' ||
      'cancelled' => backgroundLabel(run['status']),
      _ => null,
    };
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        ToolRow(
          key: ValueKey('child-${run['session']}'),
          leading: (_) =>
              Identicon(seed: text(run['session']), running: running),
          label: context.tr('conversationChild'),
          detail: status == null ? title : '${context.tr(status)} · $title',
          tone: failedRun(run) ? RowTone.failed : RowTone.normal,
          running: running,
          action: onOpen == null
              ? null
              : AppIcon(
                  'chevron',
                  size: 12,
                  color: Theme.of(context).colorScheme.onSurfaceVariant,
                ),
          onTap: onOpen,
        ),
        if (run['error'] != null)
          ErrorDetails(
            key: PageStorageKey('child-error-${run['turn']}'),
            reason: runFailure(run['error'], translate: context.tr),
          ),
      ],
    );
  }
}
