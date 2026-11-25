import 'package:flutter/material.dart';
import 'package:flutter_slidable/flutter_slidable.dart';

import '../../l10n/strings.dart';
import '../../runtime/json.dart';
import '../../runtime/session.dart';
import '../../ui/kit.dart';
import '../../ui/swipe_action.dart';
import '../../ui/toast.dart';
import '../conversations/live/presentation.dart' show failureLabel;
import '../conversations/live/terminal_tasks.dart';
import '../resources/contributions.dart';
import '../resources/overview_cards.dart';
import 'terminal_page.dart';

/// Task filter listing only terminals; also the title of their task section.
const terminalTaskScope = 'terminal';

const terminalResource = ProjectResourceFeature(
  entry: _terminalEntry,
  action: _terminalAction,
  hostCount: HostResourceCount(
    id: 'terminals',
    label: 'terminal',
    count: _terminalCount,
    open: _browseTerminals,
  ),
  taskSection: TaskListSection(
    scope: terminalTaskScope,
    records: _terminalRecords,
    include: _includeTerminal,
    row: _terminalRow,
  ),
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

List<Map<String, dynamic>> _terminalRecords(HostConnection host) =>
    objects(host.snapshot['terminals']);

int _terminalCount(HostConnection host) => _terminalRecords(host).length;

VoidCallback? _browseTerminals(BuildContext context, HostResourceView view) =>
    switch (view.browseTasks) {
      final browse? => () => browse(const TaskBrowse.filter(terminalTaskScope)),
      null => null,
    };

bool _includeTerminal(
  HostConnection host,
  Map<String, dynamic> terminal,
  TaskSectionQuery query,
) {
  final project = objects(
    host.snapshot['worktrees'],
  ).where((tree) => tree['id'] == terminal['worktree']).firstOrNull?['project'];
  return activeTerminal(terminal) &&
      query.includesProject(project) &&
      (query.filter == 'all' ||
          query.filter == terminalTaskScope ||
          terminalStatus(terminal) == query.filter) &&
      terminalTitle(
        terminal,
        translate: query.translate,
      ).toLowerCase().contains(query.search.toLowerCase());
}

Widget _terminalRow(
  BuildContext context,
  Key key,
  HostConnection host,
  Map<String, dynamic> terminal, {
  required bool divider,
}) {
  final colors = Theme.of(context).colorScheme;
  return Slidable(
    key: key,
    endActionPane: ActionPane(
      motion: const DrawerMotion(),
      extentRatio: .24,
      children: [
        SwipeAction(
          onPressed: host.connected
              ? (_) => _close(context, host, terminal)
              : null,
          color: colors.error,
          foreground: colors.onError,
          icon: Icons.close,
          label: context.tr('close'),
        ),
      ],
    ),
    child: TerminalTask(host: host, terminal: terminal, divider: divider),
  );
}

Future<void> _close(
  BuildContext context,
  HostConnection host,
  Map<String, dynamic> terminal,
) async {
  try {
    await closeTerminal(host, terminal);
  } catch (error) {
    if (context.mounted) {
      showToast(context, failureLabel(error, translate: context.tr));
    }
  }
}
