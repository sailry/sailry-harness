import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../ui/kit.dart';

typedef CommitChoice = ({bool skipHooks, bool push});

/// Floating composer-style bar shown while staged changes exist.
class CommitBar extends StatelessWidget {
  const CommitBar({
    super.key,
    required this.controller,
    required this.count,
    required this.choice,
    required this.onCommit,
    required this.onChoice,
  });

  /// Owned by the page so a failed commit keeps the message.
  final TextEditingController controller;
  final int count;
  final CommitChoice choice;

  /// Null while the page is busy or cannot write.
  final VoidCallback? onCommit;
  final ValueChanged<CommitChoice> onChoice;

  Future<void> _options(BuildContext context) async {
    var current = choice;
    await showAppSheet<void>(
      context,
      context.tr('gitCommitOptions'),
      child: StatefulBuilder(
        builder: (context, setState) => Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            SwitchListTile(
              title: Text(context.tr('gitSkipHooks')),
              value: current.skipHooks,
              onChanged: (value) {
                setState(
                  () => current = (skipHooks: value, push: current.push),
                );
                onChoice(current);
              },
            ),
            SwitchListTile(
              title: Text(context.tr('gitPushAfterCommit')),
              value: current.push,
              onChanged: (value) {
                setState(
                  () => current = (skipHooks: current.skipHooks, push: value),
                );
                onChoice(current);
              },
            ),
          ],
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final customized = choice.skipHooks || choice.push;
    return Padding(
      padding: EdgeInsets.fromLTRB(
        12,
        6,
        12,
        MediaQuery.paddingOf(context).bottom + 6,
      ),
      child: Surface(
        radius: 28,
        padding: const EdgeInsets.all(6),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.end,
          children: [
            IconButton(
              tooltip: context.tr('gitCommitOptions'),
              onPressed: () => _options(context),
              icon: AppIcon(
                'settings',
                size: 18,
                color: customized ? colors.onSurface : colors.onSurfaceVariant,
              ),
            ),
            Expanded(
              child: TextField(
                controller: controller,
                enabled: onCommit != null,
                minLines: 1,
                maxLines: 4,
                style: const TextStyle(fontSize: 15, height: 1.5),
                textInputAction: TextInputAction.newline,
                decoration: InputDecoration(
                  hintText: context.tr('commitMessage'),
                  hintMaxLines: 1,
                  border: InputBorder.none,
                  enabledBorder: InputBorder.none,
                  focusedBorder: InputBorder.none,
                  disabledBorder: InputBorder.none,
                  filled: false,
                  isDense: true,
                  contentPadding: const EdgeInsets.symmetric(
                    horizontal: 4,
                    vertical: 9,
                  ),
                ),
              ),
            ),
            const SizedBox(width: 6),
            ValueListenableBuilder(
              valueListenable: controller,
              builder: (context, value, _) => FilledButton(
                style: FilledButton.styleFrom(
                  minimumSize: const Size(0, 40),
                  padding: const EdgeInsets.symmetric(horizontal: 16),
                ),
                onPressed: onCommit != null && value.text.trim().isNotEmpty
                    ? onCommit
                    : null,
                child: Text(
                  context.tr('gitCommitCount').replaceAll('{count}', '$count'),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
