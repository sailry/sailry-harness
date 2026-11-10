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

Future<void> closeTerminal(
  HostConnection host,
  Map<String, dynamic> terminal,
) async {
  final ssh = terminal['ssh'] != null;
  await host.command(ssh ? 'close_ssh_terminal' : 'close_terminal', {
    if (!ssh) 'worktree': terminal['worktree'],
    'terminal': terminal['id'],
  });
}

class TerminalTask extends StatelessWidget {
  const TerminalTask({
    super.key,
    required this.host,
    required this.terminal,
    this.divider = true,
  });
  final HostConnection host;
  final Map<String, dynamic> terminal;
  final bool divider;

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
    // Terminals carry no Node timestamp or unread state; only live and
    // failed processes get a dot.
    final tone = !host.connected
        ? null
        : status == 'running'
        ? StatusTone.running
        : failed
        ? StatusTone.danger
        : null;
    return TaskRow(
      key: ValueKey('terminal-task-${terminal['id']}'),
      icon: const AppIcon('terminal', size: 26),
      title: terminalTitle(terminal, translate: context.tr),
      preview: text(project?['name'], context.tr('terminal')),
      status: context.tr(failed ? 'conversationFailedStatus' : 'running'),
      tone: tone,
      divider: divider,
      onTap: () => pushPage(
        context,
        TerminalPage(
          hostId: host.id,
          terminalId: text(terminal['id']),
          worktreeId: terminal['worktree'] as String?,
          title: terminalTitle(terminal, translate: context.tr),
        ),
      ),
    );
  }
}
