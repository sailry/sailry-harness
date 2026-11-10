import 'package:flutter/material.dart';

import '../../../content/diff.dart';
import '../../../content/paths.dart';
import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/kit.dart';
import 'branch_card.dart';
import 'change_row.dart';
import 'model.dart';

enum DiffAction { toggle, discard }

/// Bottom sheet with one file's unified diff and its index actions.
Future<DiffAction?> showDiffSheet(
  BuildContext context, {
  required Entry entry,
  required bool staged,
  required Map<String, dynamic> diff,
  required bool enabled,
}) {
  final path = text(entry['path']);
  return showAppSheet<DiffAction>(
    context,
    path,
    scroll: false,
    child: Builder(
      builder: (context) {
        final colors = Theme.of(context).colorScheme;
        final parts = splitPath(path);
        final discard = discardKind(entry) != DiscardKind.none;
        return Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              spacing: 10,
              children: [
                StatusBadge(changeStatus(entry, staged: staged)),
                Expanded(
                  child: Text(
                    parts.name,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(
                      fontSize: 15,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                ),
                LineCounts((
                  additions: number(diff['additions']).toInt(),
                  deletions: number(diff['deletions']).toInt(),
                ), size: 13),
              ],
            ),
            if (parts.directory.isNotEmpty)
              Padding(
                padding: const EdgeInsets.only(top: 4),
                child: Text(
                  parts.directory,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    fontFamily: 'monospace',
                    fontSize: 12,
                    color: colors.onSurfaceVariant.withValues(alpha: .65),
                  ),
                ),
              ),
            const SizedBox(height: 12),
            Flexible(
              child: ClipRRect(
                borderRadius: BorderRadius.circular(14),
                child: ColoredBox(
                  color: innerFill(context),
                  child: UnifiedDiff(diff: diff),
                ),
              ),
            ),
            if (diff['truncated'] == true)
              Padding(
                padding: const EdgeInsets.only(top: 8),
                child: Text(context.tr('resourcePartial')),
              ),
            const SizedBox(height: 14),
            Row(
              spacing: 10,
              children: [
                if (discard)
                  Expanded(
                    child: TextButton(
                      style: TextButton.styleFrom(
                        foregroundColor: colors.error,
                        backgroundColor: innerFill(context),
                        minimumSize: const Size(0, 42),
                        shape: const StadiumBorder(),
                      ),
                      onPressed: enabled
                          ? () => Navigator.pop(context, DiffAction.discard)
                          : null,
                      child: Text(context.tr('discard')),
                    ),
                  ),
                Expanded(
                  child: FilledButton(
                    style: FilledButton.styleFrom(
                      minimumSize: const Size(0, 42),
                    ),
                    onPressed: enabled
                        ? () => Navigator.pop(context, DiffAction.toggle)
                        : null,
                    child: Text(
                      context.tr(staged ? 'unstage' : 'gitStageFile'),
                    ),
                  ),
                ),
              ],
            ),
          ],
        );
      },
    ),
  );
}
