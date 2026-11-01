import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/kit.dart';
import 'model.dart';

/// Flat branch list: local branches, a remote group, then create actions.
class BranchList extends StatelessWidget {
  const BranchList({
    super.key,
    required this.branches,
    required this.truncated,
    required this.onBranch,
    required this.onCreate,
    required this.onWorktree,
  });

  final List<Entry> branches;
  final bool truncated;

  /// Null while branch actions are unavailable.
  final ValueChanged<Entry>? onBranch;
  final VoidCallback? onCreate;
  final VoidCallback? onWorktree;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final dim = colors.onSurfaceVariant.withValues(alpha: .65);
    final groups = splitBranches(branches);
    Widget row({
      required String icon,
      required String label,
      String? detail,
      Widget? trailing,
      VoidCallback? onTap,
      bool divider = true,
      bool action = false,
    }) => Opacity(
      opacity: action && onTap == null ? .45 : 1,
      child: InkWell(
        onTap: onTap,
        child: DecoratedBox(
          decoration: BoxDecoration(
            border: divider
                ? Border(bottom: BorderSide(color: colors.outlineVariant))
                : null,
          ),
          child: Padding(
            padding: const EdgeInsets.symmetric(vertical: 12),
            child: Row(
              children: [
                AppIcon(icon, size: 17, color: colors.onSurfaceVariant),
                const SizedBox(width: 12),
                Expanded(
                  child: Text.rich(
                    TextSpan(
                      text: label,
                      children: [
                        if (detail != null)
                          TextSpan(
                            text: '  $detail',
                            style: TextStyle(fontSize: 12, color: dim),
                          ),
                      ],
                    ),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(fontSize: 14),
                  ),
                ),
                ?trailing,
              ],
            ),
          ),
        ),
      ),
    );
    Widget branch(Entry entry, {required bool last}) {
      final current = entry['current'] == true;
      final remote = entry['remote'] == true;
      return row(
        icon: 'branch',
        label: text(entry['name']),
        detail: current ? context.tr('gitCurrentShort') : null,
        trailing: current
            ? AppIcon('check', size: 16, color: colors.onSurface)
            : remote
            ? null
            : AppIcon('chevron', size: 14, color: dim),
        onTap: current || remote || onBranch == null
            ? null
            : () => onBranch!(entry),
        divider: !last,
      );
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        for (final (index, entry) in groups.local.indexed)
          branch(entry, last: index == groups.local.length - 1),
        if (groups.remote.isNotEmpty) ...[
          Padding(
            padding: const EdgeInsets.only(top: 18, bottom: 2),
            child: Text(
              context.tr('gitRemote'),
              style: TextStyle(fontSize: 13, color: dim),
            ),
          ),
          for (final (index, entry) in groups.remote.indexed)
            branch(entry, last: index == groups.remote.length - 1),
        ],
        if (truncated)
          Padding(
            padding: const EdgeInsets.only(top: 8),
            child: Text(
              context.tr('resourcePartial'),
              style: TextStyle(fontSize: 12, color: dim),
            ),
          ),
        const SizedBox(height: 18),
        row(
          icon: 'plus',
          label: context.tr('gitCreateBranch'),
          onTap: onCreate,
          action: true,
        ),
        row(
          icon: 'branch',
          label: context.tr('resourceWorktreeCreate'),
          onTap: onWorktree,
          divider: false,
          action: true,
        ),
      ],
    );
  }
}
