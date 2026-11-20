import 'package:flutter/material.dart';
import 'package:flutter_slidable/flutter_slidable.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/notices.dart';
import '../../../runtime/session.dart';
import '../../../ui/kit.dart';
import '../../../ui/swipe_action.dart';
import '../../../ui/toast.dart';
import '../../terminal/terminal_page.dart';
import 'terminal_tasks.dart';

/// Opens the terminals of a conversation's worktree. A conversation without
/// a project runs in its own scratch worktree, so its terminals start there.
void showConversationTerminals(
  BuildContext context,
  HostConnection host,
  String worktree,
) => showAppSheet<void>(
  context,
  context.tr('terminal'),
  child: ConversationTerminals(host: host, worktree: worktree),
);

/// Node terminals carry their worktree but not the conversation that opened
/// them, so this lists every running shell in the conversation's worktree,
/// whichever controller started it.
class ConversationTerminals extends StatelessWidget {
  const ConversationTerminals({
    super.key,
    required this.host,
    required this.worktree,
  });
  final HostConnection host;
  final String worktree;

  Future<void> _close(
    BuildContext context,
    Map<String, dynamic> terminal,
  ) async {
    try {
      await closeTerminal(host, terminal);
    } catch (error) {
      if (context.mounted) {
        showToast(context, failureText(error, translate: context.tr));
      }
    }
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: host,
    builder: (context, _) {
      final terminals = worktreeTerminals(host, worktree);
      final colors = Theme.of(context).colorScheme;
      return Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (terminals.isEmpty)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 16),
              child: Text(
                context.tr('terminalNone'),
                textAlign: TextAlign.center,
                style: TextStyle(color: colors.onSurfaceVariant),
              ),
            ),
          for (final (index, terminal) in terminals.indexed)
            Slidable(
              key: ValueKey(('conversation-terminal', terminal['id'])),
              endActionPane: ActionPane(
                motion: const DrawerMotion(),
                extentRatio: .24,
                children: [
                  SwipeAction(
                    onPressed: host.connected
                        ? (_) => _close(context, terminal)
                        : null,
                    color: colors.error,
                    foreground: colors.onError,
                    icon: Icons.close,
                    label: context.tr('terminalEnd'),
                  ),
                ],
              ),
              child: TerminalTask(
                host: host,
                terminal: terminal,
                divider: index < terminals.length - 1,
              ),
            ),
          const SizedBox(height: 12),
          FilledButton.icon(
            key: const ValueKey('conversation-terminal-new'),
            icon: const AppIcon('terminal', size: 18),
            label: Text(context.tr('newTerminal')),
            onPressed: host.connected
                ? () => pushPage(
                    context,
                    TerminalPage(
                      hostId: host.id,
                      worktreeId: worktree,
                      createNew: true,
                    ),
                  )
                : null,
          ),
        ],
      );
    },
  );
}
