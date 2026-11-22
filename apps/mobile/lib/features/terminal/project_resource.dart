import 'package:flutter/widgets.dart';

import '../../l10n/strings.dart';
import '../../runtime/json.dart';
import '../../ui/kit.dart';
import '../conversations/live/terminal_tasks.dart';
import '../resources/contributions.dart';
import '../resources/overview_cards.dart';
import 'terminal_page.dart';

const terminalResource = ProjectResourceFeature(
  entry: _terminalEntry,
  action: _terminalAction,
);

void _open(BuildContext context, ProjectResourceView view) => pushPage<void>(
  context,
  TerminalPage(hostId: view.host.id, worktreeId: view.worktree),
);

Widget _terminalAction(BuildContext context, ProjectResourceView view) =>
    RoundButton(
      icon: 'terminal',
      tooltip: context.tr('terminal'),
      onPressed: () => _open(context, view),
    );

ResourceEntry _terminalEntry(BuildContext context, ProjectResourceView view) {
  final running = objects(view.host.snapshot['terminals'])
      .where(
        (terminal) =>
            terminal['worktree'] == view.worktree &&
            terminalStatus(terminal) == 'running',
      )
      .length;
  return (
    icon: 'terminal',
    label: context.tr('terminal'),
    detail: running == 0
        ? null
        : context.tr('terminalRunningCount').replaceAll('{count}', '$running'),
    onTap: () => _open(context, view),
  );
}
