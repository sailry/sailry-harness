import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../runtime/json.dart';
import '../../runtime/notices.dart';
import '../../runtime/session.dart';
import '../../ui/kit.dart';
import '../../ui/toast.dart';

bool activeTerminal(Map<String, dynamic> terminal) =>
    object(terminal['status'])['kind'] == 'running';

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

/// Running user terminals of one worktree, the shells a conversation in that
/// worktree can return to.
List<Map<String, dynamic>> worktreeTerminals(
  HostConnection host,
  String worktree,
) => objects(host.snapshot['terminals'])
    .where(
      (terminal) =>
          terminal['worktree'] == worktree &&
          terminal['tool'] == null &&
          terminal['ssh'] == null &&
          activeTerminal(terminal),
    )
    .toList();

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

/// Asks before ending a terminal's shell on its Node and reports whether it
/// ended. A failure stays in the sheet's toast and leaves the terminal listed.
Future<bool> confirmTerminalEnd(
  BuildContext context,
  HostConnection host,
  Map<String, dynamic> terminal,
  String title,
) async {
  var pending = false;
  final ended = await showAppSheet<bool>(
    context,
    context.tr('terminalEnd'),
    child: StatefulBuilder(
      builder: (sheet, setState) => Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(title, style: Theme.of(sheet).textTheme.titleMedium),
          const SizedBox(height: 8),
          Text(sheet.tr('terminalEndWarning')),
          const SizedBox(height: 20),
          FilledButton(
            key: const ValueKey('terminal-end-confirm'),
            style: FilledButton.styleFrom(
              backgroundColor: Theme.of(sheet).colorScheme.error,
              foregroundColor: Theme.of(sheet).colorScheme.onError,
            ),
            onPressed: pending || !host.connected
                ? null
                : () async {
                    setState(() => pending = true);
                    try {
                      await closeTerminal(host, terminal);
                      if (sheet.mounted) Navigator.pop(sheet, true);
                    } catch (error) {
                      if (!sheet.mounted) return;
                      setState(() => pending = false);
                      showToast(sheet, failureText(error, translate: sheet.tr));
                    }
                  },
            child: Text(sheet.tr('terminalEnd')),
          ),
        ],
      ),
    ),
  );
  return ended == true;
}
