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
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 12),
      child: Material(
        color: colors.surfaceContainerHigh,
        clipBehavior: Clip.antiAlias,
        borderRadius: const BorderRadius.vertical(top: Radius.circular(16)),
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxHeight: 132),
          child: ListView.separated(
            padding: EdgeInsets.zero,
            primary: false,
            shrinkWrap: true,
            itemCount: attachments.length,
            separatorBuilder: (_, _) => Divider(
              height: 1,
              indent: 10,
              endIndent: 10,
              color: colors.outlineVariant,
            ),
            itemBuilder: (context, index) {
              final item = attachments[index];
              final name = text(object(item.attachment['spec'])['name']);
              return Padding(
                padding: const EdgeInsets.only(left: 10, right: 2),
                child: SizedBox(
                  height: 44,
                  child: Row(
                    children: [
                      SizedBox.square(
                        dimension: 28,
                        child: item.preview == null
                            ? const AppIcon('file', size: 18)
                            : ClipRRect(
                                borderRadius: BorderRadius.circular(5),
                                child: Image.memory(
                                  item.preview!,
                                  cacheWidth: 84,
                                  fit: BoxFit.cover,
                                  errorBuilder: (_, _, _) =>
                                      const AppIcon('file', size: 18),
                                ),
                              ),
                      ),
                      const SizedBox(width: 8),
                      Expanded(
                        child: Text(
                          name.isEmpty ? tr('conversationAttachment') : name,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: Theme.of(context).textTheme.bodySmall,
                        ),
                      ),
                      IconButton(
                        tooltip: tr('removeAttachment'),
                        icon: const AppIcon('close', size: 16),
                        onPressed: () => onRemove(item),
                      ),
                    ],
                  ),
                ),
              );
            },
          ),
        ),
      ),
    );
  }
}
