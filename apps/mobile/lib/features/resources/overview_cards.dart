import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../runtime/json.dart';
import '../../ui/kit.dart';
import 'git/branch_card.dart';
import 'git/change_row.dart';
import 'git/model.dart';
import 'git/summary.dart';

/// Branch, sync and the first changes; hidden outside a Git repository.
class GitOverviewCard extends StatelessWidget {
  const GitOverviewCard({
    super.key,
    required this.summary,
    required this.onOpen,
  });

  final GitSummary summary;
  final VoidCallback onOpen;

  @override
  Widget build(BuildContext context) {
    final status = summary.status;
    if (status == null || !summary.repository) return const SizedBox.shrink();
    final colors = Theme.of(context).colorScheme;
    final entries = objects(status['entries']);
    final total = totalStats(entries);
    return Padding(
      padding: const EdgeInsets.only(top: 14),
      child: Surface(
        padding: const EdgeInsets.fromLTRB(12, 14, 12, 12),
        onTap: onOpen,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            BranchHeader(
              branch: text(status['branch'], shortHash(text(status['head']))),
              sync: summary.sync,
            ),
            if (entries.isEmpty)
              Padding(
                padding: const EdgeInsets.fromLTRB(4, 0, 4, 2),
                child: Text(
                  context.tr('noChanges'),
                  style: TextStyle(
                    fontSize: 13,
                    color: colors.onSurfaceVariant,
                  ),
                ),
              )
            else ...[
              DecoratedBox(
                decoration: BoxDecoration(
                  color: innerFill(context),
                  borderRadius: BorderRadius.circular(14),
                ),
                child: Padding(
                  padding: const EdgeInsets.symmetric(
                    horizontal: 12,
                    vertical: 2,
                  ),
                  child: Column(
                    children: [
                      for (final (index, entry) in entries.take(3).indexed)
                        ChangeRow(
                          entry: entry,
                          staged: null,
                          divider: index < entries.take(3).length - 1,
                        ),
                    ],
                  ),
                ),
              ),
              Padding(
                padding: const EdgeInsets.only(top: 10, bottom: 2),
                child: Row(
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: [
                    Flexible(
                      child: Text(
                        context
                            .tr('gitViewAll')
                            .replaceAll('{count}', '${entries.length}'),
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                          fontSize: 13,
                          color: colors.onSurfaceVariant,
                        ),
                      ),
                    ),
                    const SizedBox(width: 10),
                    LineCounts(total),
                  ],
                ),
              ),
            ],
          ],
        ),
      ),
    );
  }
}

/// One entry row of [ResourceEntries].
typedef ResourceEntry = ({
  String icon,
  String label,
  String? detail,
  VoidCallback onTap,
});

/// Files, terminal and ports entry rows in one card.
class ResourceEntries extends StatelessWidget {
  const ResourceEntries({super.key, required this.entries});
  final List<ResourceEntry> entries;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final dim = colors.onSurfaceVariant.withValues(alpha: .65);
    return Surface(
      padding: EdgeInsets.zero,
      child: Column(
        children: [
          for (final (index, entry) in entries.indexed)
            InkWell(
              onTap: entry.onTap,
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: 16),
                child: DecoratedBox(
                  decoration: BoxDecoration(
                    border: index < entries.length - 1
                        ? Border(
                            bottom: BorderSide(color: colors.outlineVariant),
                          )
                        : null,
                  ),
                  child: Padding(
                    padding: const EdgeInsets.symmetric(vertical: 13),
                    child: Row(
                      children: [
                        AppIcon(
                          entry.icon,
                          size: 19,
                          color: colors.onSurfaceVariant,
                        ),
                        const SizedBox(width: 12),
                        Expanded(
                          child: Text(
                            entry.label,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: const TextStyle(fontSize: 15),
                          ),
                        ),
                        if (entry.detail != null) ...[
                          const SizedBox(width: 8),
                          Text(
                            entry.detail!,
                            style: TextStyle(fontSize: 13, color: dim),
                          ),
                        ],
                        const SizedBox(width: 8),
                        AppIcon('chevron', size: 15, color: dim),
                      ],
                    ),
                  ),
                ),
              ),
            ),
        ],
      ),
    );
  }
}
