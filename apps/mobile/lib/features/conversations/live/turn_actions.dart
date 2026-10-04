import 'package:flutter/material.dart';
import '../../../content/code.dart';
import '../../../l10n/strings.dart';
import '../../../ui/kit.dart';

class TurnActions extends StatelessWidget {
  const TurnActions({
    super.key,
    required this.copy,
    this.onAction,
    this.failed = false,
    this.canRewind = false,
    this.canEdit = true,
  });
  final String copy;
  final ValueChanged<String>? onAction;
  final bool failed, canRewind, canEdit;
  @override
  Widget build(BuildContext context) => Row(
    children: [
      if (copy.isNotEmpty) CopyTextButton(copy),
      if (onAction != null) ...[
        if (failed && canEdit)
          IconButton(
            tooltip: tr('retry'),
            icon: const AppIcon('refresh', size: 16),
            onPressed: () => onAction!('retry'),
          ),
        IconButton(
          tooltip: tr('messageActions'),
          icon: const AppIcon('more', size: 16),
          onPressed: () => showAppSheet(
            context,
            tr('messageActions'),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                for (final action in [
                  if (canEdit) 'edit',
                  'fork',
                  if (canRewind) 'rewind',
                ])
                  ListTile(
                    title: Text(
                      tr(switch (action) {
                        'edit' => 'messageEdit',
                        'fork' => 'fork',
                        _ => 'messageRewind',
                      }),
                    ),
                    onTap: () {
                      Navigator.pop(context);
                      onAction!(action);
                    },
                  ),
              ],
            ),
          ),
        ),
      ],
    ],
  );
}
