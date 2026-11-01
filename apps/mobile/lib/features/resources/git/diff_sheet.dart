import 'package:flutter/material.dart';

import '../../../content/diff.dart';
import '../../../content/paths.dart';
import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/kit.dart';
import '../../../ui/theme.dart';
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

/// Unified diff rows with one line-number gutter and tinted changes.
class UnifiedDiff extends StatelessWidget {
  const UnifiedDiff({super.key, required this.diff});
  final Map<String, dynamic> diff;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final lines = hunkLines(
      text(diff['text']),
      added: diff['format'] == 'added',
    );
    final digits = lines
        .map((line) => '${line.after ?? line.before ?? ''}'.length)
        .fold(1, (width, value) => value > width ? value : width);
    final style = TextStyle(
      fontFamily: 'monospace',
      fontSize: 12.5,
      height: 1.6,
      color: colors.onSurface,
    );
    final blue = SailryTheme.accentColor(context, AccentTone.blue);
    final added = SailryTheme.statusColor(context, StatusTone.success);
    final removed = SailryTheme.statusColor(context, StatusTone.danger);
    return LayoutBuilder(
      builder: (context, constraints) => SingleChildScrollView(
        padding: const EdgeInsets.symmetric(vertical: 8),
        child: SingleChildScrollView(
          scrollDirection: Axis.horizontal,
          child: ConstrainedBox(
            constraints: BoxConstraints(minWidth: constraints.maxWidth),
            child: IntrinsicWidth(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  for (final line in lines)
                    ColoredBox(
                      color: switch (line.kind) {
                        'header' => blue.withValues(alpha: .08),
                        'added' => added.withValues(alpha: .12),
                        'removed' => removed.withValues(alpha: .12),
                        _ => Colors.transparent,
                      },
                      child: Padding(
                        padding: const EdgeInsets.symmetric(horizontal: 12),
                        child: Row(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(
                              '${line.after ?? line.before ?? ''}'.padLeft(
                                digits,
                              ),
                              style: style.copyWith(
                                color: colors.onSurfaceVariant.withValues(
                                  alpha: .55,
                                ),
                              ),
                            ),
                            const SizedBox(width: 12),
                            Text(
                              line.text,
                              softWrap: false,
                              style: line.kind == 'header'
                                  ? style.copyWith(color: blue)
                                  : style,
                            ),
                          ],
                        ),
                      ),
                    ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}
