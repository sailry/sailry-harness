import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../l10n/strings.dart';
import '../../ui/kit.dart';
import '../../ui/theme.dart';
import '../../ui/toast.dart';

class UserBubble extends StatelessWidget {
  const UserBubble({
    super.key,
    required this.text,
    this.attachment,
    this.time,
    this.onEdit,
  });

  final String text;
  final Widget? attachment;
  final String? time;
  final VoidCallback? onEdit;

  Future<void> _copy(BuildContext context) async {
    var notice = 'copied';
    try {
      await Clipboard.setData(ClipboardData(text: text));
    } catch (_) {
      notice = 'copyFailed';
    }
    if (context.mounted) {
      showToast(context, tr(notice));
    }
  }

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.only(top: 12, bottom: 24),
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.end,
      children: [
        LayoutBuilder(
          builder: (context, constraints) => Align(
            alignment: Alignment.centerRight,
            child: ConstrainedBox(
              constraints: BoxConstraints(maxWidth: constraints.maxWidth * .88),
              child: Material(
                color: SailryTheme.userMessageFill(context),
                borderRadius: const BorderRadius.only(
                  topLeft: Radius.circular(18),
                  topRight: Radius.circular(18),
                  bottomLeft: Radius.circular(18),
                  bottomRight: Radius.circular(5),
                ),
                clipBehavior: Clip.antiAlias,
                child: Padding(
                  padding: const EdgeInsets.symmetric(
                    horizontal: 15,
                    vertical: 10,
                  ),
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      if (text.isNotEmpty)
                        SelectionArea(
                          child: Text(
                            text,
                            style: const TextStyle(fontSize: 14, height: 1.5),
                          ),
                        ),
                      if (attachment != null)
                        Padding(
                          padding: EdgeInsets.only(top: text.isEmpty ? 0 : 10),
                          child: attachment,
                        ),
                    ],
                  ),
                ),
              ),
            ),
          ),
        ),
        const SizedBox(height: 6),
        Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (time != null) ...[
              Text(
                time!,
                style: TextStyle(
                  color: Theme.of(context).colorScheme.onSurfaceVariant,
                ),
              ),
              const SizedBox(width: 5),
            ],
            if (onEdit != null)
              IconButton(
                tooltip: tr('messageEdit'),
                onPressed: onEdit,
                icon: const AppIcon('edit', size: 14),
              ),
            IconButton(
              tooltip: tr('copy'),
              onPressed: () => _copy(context),
              style: IconButton.styleFrom(
                padding: const EdgeInsets.all(6),
                minimumSize: const Size.square(28),
                tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                visualDensity: VisualDensity.standard,
              ),
              icon: const AppIcon('copy', size: 14),
            ),
          ],
        ),
      ],
    ),
  );
}

class SentAttachment extends StatelessWidget {
  const SentAttachment({
    super.key,
    required this.name,
    required this.onPressed,
  });

  final String name;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return OutlinedButton(
      onPressed: onPressed,
      style: OutlinedButton.styleFrom(
        foregroundColor: colors.onSurfaceVariant,
        backgroundColor: colors.surfaceContainer,
        side: BorderSide(color: colors.outlineVariant),
        padding: const EdgeInsets.symmetric(horizontal: 9, vertical: 6),
        minimumSize: Size.zero,
        tapTargetSize: MaterialTapTargetSize.shrinkWrap,
        visualDensity: VisualDensity.standard,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(9)),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          const AppIcon('file', size: 15),
          const SizedBox(width: 6),
          Flexible(
            child: Text(name, maxLines: 1, overflow: TextOverflow.ellipsis),
          ),
        ],
      ),
    );
  }
}
