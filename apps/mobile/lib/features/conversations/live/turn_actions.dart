import 'package:flutter/material.dart';
import '../../../content/code.dart';
import '../../../l10n/strings.dart';
import '../../../ui/kit.dart';

/// Message actions for a settled turn; [copy] adds Copy for long-pressed turns.
void showTurnActions(
  BuildContext context, {
  required ValueChanged<String>? onAction,
  String copy = '',
  bool canRewind = false,
  bool canEdit = true,
}) => showAppSheet(
  context,
  context.tr('messageActions'),
  child: Column(
    mainAxisSize: MainAxisSize.min,
    children: [
      for (final action in [
        if (copy.isNotEmpty) 'copy',
        if (onAction != null) ...[
          if (canEdit) 'edit',
          'fork',
          if (canRewind) 'rewind',
        ],
      ])
        ListTile(
          title: Text(
            context.tr(switch (action) {
              'copy' => 'copy',
              'edit' => 'messageEdit',
              'fork' => 'fork',
              _ => 'messageRewind',
            }),
          ),
          onTap: () {
            Navigator.pop(context);
            if (action == 'copy') {
              copyText(context, copy);
            } else {
              onAction!(action);
            }
          },
        ),
    ],
  ),
);

/// Compact actions under the latest answer, starting at the text's edge.
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

  static final _style = IconButton.styleFrom(
    padding: const EdgeInsets.all(6),
    minimumSize: const Size.square(28),
    fixedSize: const Size.square(28),
    tapTargetSize: MaterialTapTargetSize.shrinkWrap,
    visualDensity: VisualDensity.compact,
  );

  @override
  Widget build(BuildContext context) => Transform.translate(
    // The icon, not its tap padding, aligns with the answer text.
    offset: const Offset(-6, 0),
    child: Row(
      spacing: 4,
      children: [
        if (onAction != null && failed && canEdit)
          IconButton(
            style: _style,
            tooltip: context.tr('retry'),
            icon: const AppIcon('refresh', size: 16),
            onPressed: () => onAction!('retry'),
          ),
        if (copy.isNotEmpty) CopyTextButton(copy, style: _style),
        if (onAction != null)
          IconButton(
            style: _style,
            tooltip: context.tr('messageActions'),
            icon: const AppIcon('ellipsis', size: 16),
            onPressed: () => showTurnActions(
              context,
              onAction: onAction,
              canRewind: canRewind,
              canEdit: canEdit,
            ),
          ),
      ],
    ),
  );
}
