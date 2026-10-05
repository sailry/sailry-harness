import 'package:flutter/material.dart';
import 'package:flutter_slidable/flutter_slidable.dart';

import '../../l10n/strings.dart';
import '../../ui/kit.dart';

class QueuedDraft {
  QueuedDraft(this.id, this.text);
  final int id;
  String text;
}

class QueueSheet extends StatefulWidget {
  const QueueSheet({
    super.key,
    required this.items,
    required this.paused,
    required this.busy,
    required this.onChanged,
    required this.onPause,
    required this.onSend,
  });

  final List<QueuedDraft> items;
  final bool paused;
  final bool busy;
  final VoidCallback onChanged;
  final ValueChanged<bool> onPause;
  final VoidCallback onSend;

  @override
  State<QueueSheet> createState() => _QueueSheetState();
}

class _QueueSheetState extends State<QueueSheet> {
  late bool _paused = widget.paused;

  Future<void> _edit(QueuedDraft item) async {
    var draft = item.text;
    await showAppSheet(
      context,
      context.tr('edit'),
      child: StatefulBuilder(
        builder: (context, update) => Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            TextFormField(
              initialValue: draft,
              autofocus: true,
              minLines: 3,
              maxLines: 6,
              onChanged: (value) => update(() => draft = value),
              decoration: InputDecoration(hintText: context.tr('describeTask')),
            ),
            const SizedBox(height: 16),
            SizedBox(
              width: double.infinity,
              child: FilledButton(
                onPressed: draft.trim().isEmpty
                    ? null
                    : () {
                        setState(() => item.text = draft.trim());
                        widget.onChanged();
                        Navigator.pop(context);
                      },
                child: Text(context.tr('save')),
              ),
            ),
          ],
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return SlidableAutoCloseBehavior(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (widget.items.isEmpty)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 24),
              child: Text(
                context.tr('queueEmpty'),
                style: TextStyle(color: colors.onSurfaceVariant),
              ),
            ),
          for (final item in widget.items)
            Padding(
              padding: const EdgeInsets.only(bottom: 10),
              child: ClipRRect(
                borderRadius: BorderRadius.circular(16),
                child: Slidable(
                  key: ValueKey(item.id),
                  endActionPane: ActionPane(
                    motion: const DrawerMotion(),
                    extentRatio: widget.items.indexOf(item) > 0 ? .72 : .5,
                    children: [
                      CustomSlidableAction(
                        onPressed: (_) => _edit(item),
                        backgroundColor: colors.secondaryContainer,
                        foregroundColor: colors.onSecondaryContainer,
                        child: Column(
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            AppIcon(
                              'edit',
                              size: 24,
                              color: colors.onSecondaryContainer,
                            ),
                            const SizedBox(height: 4),
                            Text(
                              context.tr('edit'),
                              overflow: TextOverflow.ellipsis,
                            ),
                          ],
                        ),
                      ),
                      CustomSlidableAction(
                        onPressed: (_) {
                          setState(() => widget.items.remove(item));
                          widget.onChanged();
                        },
                        backgroundColor: colors.error,
                        foregroundColor: colors.onError,
                        child: Column(
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            AppIcon('trash', size: 24, color: colors.onError),
                            const SizedBox(height: 4),
                            Text(
                              context.tr('delete'),
                              overflow: TextOverflow.ellipsis,
                            ),
                          ],
                        ),
                      ),
                      if (widget.items.indexOf(item) > 0)
                        CustomSlidableAction(
                          onPressed: (_) {
                            setState(() {
                              final index = widget.items.indexOf(item);
                              widget.items.removeAt(index);
                              widget.items.insert(index - 1, item);
                            });
                            widget.onChanged();
                          },
                          backgroundColor: colors.tertiaryContainer,
                          foregroundColor: colors.onTertiaryContainer,
                          child: Column(
                            mainAxisSize: MainAxisSize.min,
                            children: [
                              AppIcon(
                                'arrow-up',
                                size: 24,
                                color: colors.onTertiaryContainer,
                              ),
                              const SizedBox(height: 4),
                              Text(
                                context.tr('moveUp'),
                                overflow: TextOverflow.ellipsis,
                              ),
                            ],
                          ),
                        ),
                    ],
                  ),
                  child: Surface(
                    radius: 16,
                    onTap: () => _edit(item),
                    child: Row(
                      children: [
                        Expanded(
                          child: Text(
                            item.text,
                            style: const TextStyle(height: 1.5),
                          ),
                        ),
                        const SizedBox(width: 12),
                        AppIcon(
                          'chevron',
                          size: 16,
                          color: colors.onSurfaceVariant,
                        ),
                      ],
                    ),
                  ),
                ),
              ),
            ),
          if (widget.items.isNotEmpty) ...[
            const SizedBox(height: 10),
            Row(
              children: [
                Expanded(
                  child: OutlinedButton(
                    onPressed: () {
                      setState(() => _paused = !_paused);
                      widget.onPause(_paused);
                    },
                    child: Text(
                      context.tr(_paused ? 'resumeQueue' : 'pauseQueue'),
                    ),
                  ),
                ),
                if (!widget.busy) ...[
                  const SizedBox(width: 8),
                  Expanded(
                    child: FilledButton(
                      onPressed: () {
                        Navigator.pop(context);
                        widget.onSend();
                      },
                      child: Text(context.tr('sendNext')),
                    ),
                  ),
                ],
              ],
            ),
          ],
        ],
      ),
    );
  }
}
