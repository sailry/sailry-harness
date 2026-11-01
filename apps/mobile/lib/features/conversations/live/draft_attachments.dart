import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/kit.dart';
import 'attachments.dart';

class DraftAttachments extends StatelessWidget {
  const DraftAttachments({
    super.key,
    required this.attachments,
    required this.onRemove,
  });

  final List<PickedAttachment> attachments;
  final ValueChanged<PickedAttachment> onRemove;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    // Glass capsules in one scrolling row, matching the status chips.
    return Padding(
      padding: const EdgeInsets.only(bottom: 8),
      child: SizedBox(
        height: 40,
        child: ListView.separated(
          padding: EdgeInsets.zero,
          clipBehavior: Clip.none,
          scrollDirection: Axis.horizontal,
          primary: false,
          itemCount: attachments.length,
          separatorBuilder: (_, _) => const SizedBox(width: 8),
          itemBuilder: (context, index) {
            final item = attachments[index];
            final name = text(object(item.attachment['spec'])['name']);
            return Surface(
              radius: 20,
              padding: const EdgeInsets.only(left: 6),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  SizedBox.square(
                    dimension: 28,
                    child: item.preview == null
                        ? DecoratedBox(
                            decoration: BoxDecoration(
                              color: colors.onSurface.withValues(alpha: .06),
                              shape: BoxShape.circle,
                            ),
                            child: const Center(
                              child: AppIcon('file', size: 15),
                            ),
                          )
                        : ClipOval(
                            child: Image.memory(
                              item.preview!,
                              cacheWidth: 84,
                              fit: BoxFit.cover,
                              errorBuilder: (_, _, _) =>
                                  const AppIcon('file', size: 15),
                            ),
                          ),
                  ),
                  const SizedBox(width: 8),
                  ConstrainedBox(
                    constraints: const BoxConstraints(maxWidth: 150),
                    child: Text(
                      name.isEmpty
                          ? context.tr('conversationAttachment')
                          : name,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(fontSize: 13, color: colors.onSurface),
                    ),
                  ),
                  IconButton(
                    tooltip: context.tr('removeAttachment'),
                    visualDensity: VisualDensity.compact,
                    icon: AppIcon(
                      'close',
                      size: 14,
                      color: colors.onSurfaceVariant,
                    ),
                    onPressed: () => onRemove(item),
                  ),
                ],
              ),
            );
          },
        ),
      ),
    );
  }
}
