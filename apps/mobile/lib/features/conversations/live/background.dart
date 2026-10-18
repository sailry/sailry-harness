import 'package:flutter/material.dart';
import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/kit.dart';
import 'background_data.dart';
import 'disclosure.dart';

class BackgroundNotices extends StatelessWidget {
  const BackgroundNotices({
    super.key,
    required this.items,
    required this.children,
    this.pending = false,
    this.onOpenChild,
  });
  final List<Map<String, dynamic>> items;
  final List<Map<String, dynamic>> children;
  final bool pending;
  final ValueChanged<Map<String, dynamic>>? onOpenChild;

  Map<String, dynamic>? child(Map<String, dynamic> item) {
    final kind = object(item['kind']);
    if (kind['kind'] != 'agent') return null;
    final session = object(kind['data'])['session'];
    return children
        .where((entry) => object(entry['run'])['session'] == session)
        .firstOrNull;
  }

  @override
  Widget build(BuildContext context) {
    if (items.isEmpty) return const SizedBox.shrink();
    return WorkDisclosure(
      title: Row(
        children: [
          const AppIcon('spark', size: 18),
          const SizedBox(width: 8),
          Expanded(
            child: Text(
              context.tr(
                pending ? 'backgroundPending' : 'backgroundNoticeTitle',
              ),
            ),
          ),
          Text('${items.length}'),
        ],
      ),
      autoExpanded: !pending,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          for (final item in items) ...[
            ListTile(
              contentPadding: EdgeInsets.zero,
              title: Text(text(item['title'])),
              subtitle: Text(context.tr(backgroundLabel(item['outcome']))),
              onTap: child(item) != null && onOpenChild != null
                  ? () => onOpenChild!(child(item)!)
                  : null,
              trailing: child(item) != null && onOpenChild != null
                  ? const AppIcon('chevron')
                  : null,
            ),
            if (text(item['summary']).isNotEmpty) Text(text(item['summary'])),
          ],
        ],
      ),
    );
  }
}
