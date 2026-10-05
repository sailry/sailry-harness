import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../ui/kit.dart';
import '../task_row.dart';
import '../../../ui/theme.dart';
import '../../terminal/terminal_page.dart';

String terminalTitle(
  Map<String, dynamic> terminal, {
  Translator translate = tr,
}) => text(terminal['title']).isEmpty
    ? translate('terminal')
    : text(terminal['title']);

String terminalStatus(Map<String, dynamic> terminal) {
  final status = object(terminal['status']);
  return switch (status['kind']) {
    'running' => 'running',
    'exited' when status['code'] == 0 => 'completed',
    _ => 'idle',
  };
}

class TerminalTask extends StatelessWidget {
  const TerminalTask({super.key, required this.host, required this.terminal});
  final HostConnection host;
  final Map<String, dynamic> terminal;

  @override
  Widget build(BuildContext context) {
    final tree = objects(
      host.snapshot['worktrees'],
    ).where((tree) => tree['id'] == terminal['worktree']).firstOrNull;
    final project = objects(
      host.snapshot['projects'],
    ).where((project) => project['id'] == tree?['project']).firstOrNull;
    final status = terminalStatus(terminal);
    final kind = object(terminal['status'])['kind'];
    final failed =
        kind == 'failed' || kind == 'exited' && status != 'completed';
    return Padding(
      padding: const EdgeInsets.only(bottom: 9),
      child: TaskRow(
        key: ValueKey('terminal-task-${terminal['id']}'),
        icon: const AppIcon('terminal', size: 26),
        title: terminalTitle(terminal, translate: context.tr),
        preview: text(project?['name'], context.tr('terminal')),
        status: context.tr(
          !host.connected
              ? 'offline'
              : failed
              ? 'conversationFailedStatus'
              : status == 'idle'
              ? 'stoppedStatus'
              : status,
        ),
        tone: !host.connected
            ? StatusTone.neutral
            : status == 'completed'
            ? StatusTone.success
            : status == 'idle'
            ? StatusTone.danger
            : StatusTone.running,
        onTap: () => pushPage(
          context,
          TerminalPage(
            hostId: host.id,
            terminalId: text(terminal['id']),
            worktreeId: terminal['worktree'] as String?,
            title: terminalTitle(terminal, translate: context.tr),
          ),
        ),
      ),
    );
  }
}
