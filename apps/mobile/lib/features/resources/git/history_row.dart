import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../conversations/live/recency.dart';
import 'branch_card.dart';
import 'model.dart';

/// Two-line commit row: subject and short hash, then references, author
/// and time.
class HistoryRow extends StatelessWidget {
  const HistoryRow({
    super.key,
    required this.entry,
    required this.branch,
    required this.now,
    this.onTap,
    this.divider = true,
  });

  final Entry entry;
  final String? branch;
  final DateTime now;
  final VoidCallback? onTap;
  final bool divider;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final dim = colors.onSurfaceVariant.withValues(alpha: .65);
    final seconds = entry['timestamp'];
    final time = seconds is num
        ? timeLabel(
            DateTime.fromMillisecondsSinceEpoch(seconds.toInt() * 1000),
            now,
            locale: dateLocale(Localizations.localeOf(context)),
            yesterday: context.tr('yesterday'),
          )
        : null;
    final references = (entry['references'] as List? ?? const [])
        .whereType<String>()
        .toList();
    final meta = [
      text(entry['author']),
      ?time,
    ].where((part) => part.isNotEmpty).join('  ');
    return InkWell(
      onTap: onTap,
      child: DecoratedBox(
        decoration: BoxDecoration(
          border: divider
              ? Border(bottom: BorderSide(color: colors.outlineVariant))
              : null,
        ),
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: 11),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            spacing: 5,
            children: [
              Row(
                children: [
                  Expanded(
                    child: Text(
                      text(entry['message']).split('\n').first,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: const TextStyle(fontSize: 14),
                    ),
                  ),
                  const SizedBox(width: 10),
                  Text(
                    shortHash(text(entry['id'])),
                    style: TextStyle(
                      fontFamily: 'monospace',
                      fontSize: 12,
                      color: dim,
                    ),
                  ),
                ],
              ),
              Row(
                children: [
                  for (final reference in references.take(3)) ...[
                    _ReferencePill(
                      referenceLabel(reference),
                      current: isCurrentReference(reference, branch),
                    ),
                    const SizedBox(width: 6),
                  ],
                  Expanded(
                    child: Text(
                      meta,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(fontSize: 12, color: dim),
                    ),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _ReferencePill extends StatelessWidget {
  const _ReferencePill(this.label, {required this.current});
  final String label;
  final bool current;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return ConstrainedBox(
      constraints: const BoxConstraints(maxWidth: 120),
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 1),
        decoration: BoxDecoration(
          color: current ? colors.surfaceContainerHigh : innerFill(context),
          borderRadius: BorderRadius.circular(999),
        ),
        child: Text(
          label,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: TextStyle(
            fontSize: 11,
            color: current ? colors.onSurface : colors.onSurfaceVariant,
            fontWeight: current ? FontWeight.w500 : null,
          ),
        ),
      ),
    );
  }
}
