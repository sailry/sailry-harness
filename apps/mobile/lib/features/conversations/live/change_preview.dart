import 'package:flutter/material.dart';

import '../../../content/diff.dart';
import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/theme.dart';

String changeTitle(Translator tr, int count) => count == 1
    ? tr('fileChangesApprovalOne')
    : tr('fileChangesApproval').replaceAll('{count}', '$count');

/// A change set awaiting one decision; each Node-validated file shows its
/// bounded diff exactly as provided.
class ChangePreview extends StatelessWidget {
  const ChangePreview({super.key, required this.changes});
  final Map<String, dynamic> changes;

  @override
  Widget build(BuildContext context) {
    final files = objects(changes['files']);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      spacing: 12,
      children: [
        Text(changeTitle(context.tr, files.length)),
        for (final file in files) _File(file),
      ],
    );
  }
}

class _File extends StatelessWidget {
  const _File(this.file);
  final Map<String, dynamic> file;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final muted = TextStyle(fontSize: 12, color: colors.onSurfaceVariant);
    final kind = switch (file['kind']) {
      'added' => context.tr('fileChangesAdded'),
      'modified' => context.tr('fileChangesModified'),
      'renamed' => context.tr('fileChangesRenamed'),
      'deleted' => context.tr('fileChangesDeleted'),
      _ => null,
    };
    final destination = file['destination'];
    final path = destination is String
        ? '${text(file['path'])} → $destination'
        : text(file['path']);
    final diff = text(file['diff']);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      spacing: 6,
      children: [
        Row(
          crossAxisAlignment: CrossAxisAlignment.baseline,
          textBaseline: TextBaseline.alphabetic,
          spacing: 8,
          children: [
            if (kind != null) Text(kind, style: muted),
            Expanded(
              child: Text(
                path,
                style: const TextStyle(
                  fontFamily: 'monospace',
                  fontSize: 13,
                  height: 1.5,
                ),
              ),
            ),
            Text(
              '+${number(file['additions']).toInt()}',
              style: muted.copyWith(
                color: SailryTheme.statusColor(context, StatusTone.success),
              ),
            ),
            Text(
              '−${number(file['deletions']).toInt()}',
              style: muted.copyWith(
                color: SailryTheme.statusColor(context, StatusTone.danger),
              ),
            ),
          ],
        ),
        if (diff.isNotEmpty)
          ClipRRect(
            borderRadius: BorderRadius.circular(12),
            child: ColoredBox(
              color: colors.onSurface.withValues(alpha: .03),
              child: ConstrainedBox(
                constraints: const BoxConstraints(maxHeight: 240),
                child: UnifiedDiff(diff: {'text': diff}),
              ),
            ),
          ),
        if (file['truncated'] == true)
          Text(context.tr('resourcePartial'), style: muted),
      ],
    );
  }
}
