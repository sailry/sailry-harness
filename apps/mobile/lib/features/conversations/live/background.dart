import 'package:flutter/material.dart';

import '../../../content/code.dart';
import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/kit.dart';
import '../user_message.dart';
import 'background_data.dart';
import 'tool_row.dart';

/// Each background result reads like a message: its title and outcome.
class BackgroundNotices extends StatelessWidget {
  const BackgroundNotices({
    super.key,
    required this.items,
    required this.children,
    this.onOpenChild,
  });
  final List<Map<String, dynamic>> items;
  final Map<String, dynamic> children;
  final ValueChanged<Map<String, dynamic>>? onOpenChild;

  Map<String, dynamic>? child(Map<String, dynamic> item) {
    final kind = object(item['kind']);
    if (kind['kind'] != 'agent') return null;
    final session = object(kind['data'])['session'];
    final reference = children[session];
    return reference == null ? null : object(reference);
  }

  RowTone tone(Map<String, dynamic> item) {
    final outcome = item['outcome'];
    if (backgroundFailed(outcome)) return RowTone.failed;
    if (backgroundWarning(outcome)) return RowTone.pending;
    return RowTone.normal;
  }

  void _details(BuildContext context, Map<String, dynamic> item) {
    final summary = text(item['summary']);
    showAppSheet(
      context,
      text(item['title']),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          ToolRow(
            icon: child(item) == null ? 'clock' : 'spark',
            label: text(item['title']),
            detail: context.tr(backgroundLabel(item['outcome'])),
            tone: tone(item),
          ),
          if (summary.isNotEmpty) ...[
            const SizedBox(height: 8),
            CodeBlock(summary),
          ],
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    if (items.isEmpty) return const SizedBox.shrink();
    final colors = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.only(top: 12, bottom: 16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          for (final (index, item) in items.indexed)
            Padding(
              padding: EdgeInsets.only(top: index == 0 ? 0 : 8),
              child: MessageBubble(
                onTap: () {
                  final reference = child(item);
                  if (reference != null && onOpenChild != null) {
                    onOpenChild!(reference);
                  } else {
                    _details(context, item);
                  }
                },
                child: DefaultTextStyle.merge(
                  style: const TextStyle(
                    fontSize: transcriptFontSize,
                    height: 1.5,
                  ),
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(text(item['title'])),
                      Text(
                        context.tr(backgroundLabel(item['outcome'])),
                        style: TextStyle(
                          color: tone(item) == RowTone.normal
                              ? colors.onSurfaceVariant.withValues(alpha: .65)
                              : rowColor(context, tone(item)),
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            ),
        ],
      ),
    );
  }
}
