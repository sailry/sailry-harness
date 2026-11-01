import 'package:flutter/material.dart';

import '../../../content/paths.dart';
import '../../../runtime/json.dart';
import '../../../ui/theme.dart';
import 'model.dart';

const _tabular = [FontFeature.tabularFigures()];

Color changeColor(BuildContext context, ChangeStatus status) =>
    SailryTheme.statusColor(context, switch (status) {
      ChangeStatus.modified || ChangeStatus.renamed => StatusTone.warning,
      ChangeStatus.added => StatusTone.success,
      ChangeStatus.deleted || ChangeStatus.conflicted => StatusTone.danger,
      ChangeStatus.untracked => StatusTone.neutral,
    });

class StatusBadge extends StatelessWidget {
  const StatusBadge(this.status, {super.key});
  final ChangeStatus status;

  @override
  Widget build(BuildContext context) {
    final color = changeColor(context, status);
    return Container(
      width: 18,
      height: 18,
      alignment: Alignment.center,
      decoration: BoxDecoration(
        color: color.withValues(alpha: .14),
        borderRadius: BorderRadius.circular(5),
      ),
      child: Text(
        status.letter,
        style: TextStyle(
          fontFamily: 'monospace',
          fontSize: 11,
          height: 1,
          fontWeight: FontWeight.w600,
          color: color,
        ),
      ),
    );
  }
}

/// Added and removed line counts; empty when both are zero.
class LineCounts extends StatelessWidget {
  const LineCounts(this.stats, {super.key, this.size = 12});
  final LineStats stats;
  final double size;

  @override
  Widget build(BuildContext context) {
    if (stats.additions == 0 && stats.deletions == 0) {
      return const SizedBox.shrink();
    }
    final style = TextStyle(fontSize: size, fontFeatures: _tabular);
    return Text.rich(
      TextSpan(
        children: [
          TextSpan(
            text: '+${stats.additions}',
            style: TextStyle(
              color: SailryTheme.statusColor(context, StatusTone.success),
            ),
          ),
          const TextSpan(text: '  '),
          TextSpan(
            text: '−${stats.deletions}',
            style: TextStyle(
              color: SailryTheme.statusColor(context, StatusTone.danger),
            ),
          ),
        ],
      ),
      style: style,
      softWrap: false,
    );
  }
}

/// File name with its dim directory tail on one line.
class PathLabel extends StatelessWidget {
  const PathLabel(this.path, {super.key, this.size = 14});
  final String path;
  final double size;

  @override
  Widget build(BuildContext context) {
    final parts = splitPath(path);
    final directory = shortDirectory(parts.directory);
    return Text.rich(
      TextSpan(
        text: parts.name,
        children: [
          if (directory.isNotEmpty)
            TextSpan(
              text: '  $directory',
              style: TextStyle(
                fontSize: 12,
                color: Theme.of(
                  context,
                ).colorScheme.onSurfaceVariant.withValues(alpha: .65),
              ),
            ),
        ],
      ),
      maxLines: 1,
      overflow: TextOverflow.ellipsis,
      style: TextStyle(fontSize: size),
    );
  }
}

class ChangeRow extends StatelessWidget {
  const ChangeRow({
    super.key,
    required this.entry,
    required this.staged,
    this.onTap,
    this.divider = true,
    this.padding = EdgeInsets.zero,
  });

  final Entry entry;

  /// Null summarizes both sides, as in the overview card.
  final bool? staged;
  final VoidCallback? onTap;
  final bool divider;
  final EdgeInsetsGeometry padding;

  @override
  Widget build(BuildContext context) => InkWell(
    onTap: onTap,
    child: Padding(
      padding: padding,
      child: DecoratedBox(
        decoration: BoxDecoration(
          border: divider
              ? Border(
                  bottom: BorderSide(
                    color: Theme.of(context).colorScheme.outlineVariant,
                  ),
                )
              : null,
        ),
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: 10),
          child: Row(
            spacing: 10,
            children: [
              StatusBadge(
                changeStatus(
                  entry,
                  staged:
                      staged ??
                      entry['unstaged'] == null && entry['untracked'] != true,
                ),
              ),
              Expanded(child: PathLabel(text(entry['path']))),
              LineCounts(lineStats(entry, staged: staged)),
            ],
          ),
        ),
      ),
    ),
  );
}
