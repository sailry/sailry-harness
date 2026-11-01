import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../ui/kit.dart';

const _tabular = [FontFeature.tabularFigures()];

/// Plain fill for content nested inside a glass card.
Color innerFill(BuildContext context) =>
    Theme.of(context).colorScheme.onSurface.withValues(alpha: .045);

/// Upstream sync from `list_git_branches`.
typedef BranchSync = ({String? upstream, int ahead, int behind});

class SyncPill extends StatelessWidget {
  const SyncPill(this.sync, {super.key});
  final BranchSync sync;

  @override
  Widget build(BuildContext context) => Container(
    padding: const EdgeInsets.symmetric(horizontal: 9, vertical: 3),
    decoration: BoxDecoration(
      color: innerFill(context),
      borderRadius: BorderRadius.circular(999),
    ),
    child: Text(
      '↑${sync.ahead} ↓${sync.behind}',
      style: TextStyle(
        fontSize: 12,
        color: Theme.of(context).colorScheme.onSurfaceVariant,
        fontFeatures: _tabular,
      ),
    ),
  );
}

/// Branch icon, name, dim upstream and sync pill.
class BranchHeader extends StatelessWidget {
  const BranchHeader({
    super.key,
    required this.branch,
    required this.sync,
    this.onTap,
  });

  final String branch;
  final BranchSync? sync;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final upstream = sync?.upstream;
    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(10),
      child: Padding(
        padding: const EdgeInsets.fromLTRB(4, 2, 4, 10),
        child: Row(
          children: [
            AppIcon('branch', size: 16, color: colors.onSurfaceVariant),
            const SizedBox(width: 8),
            Flexible(
              child: Text(
                branch,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: const TextStyle(
                  fontSize: 15,
                  fontWeight: FontWeight.w600,
                ),
              ),
            ),
            if (upstream != null) ...[
              const SizedBox(width: 8),
              Flexible(
                child: Text(
                  upstream,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    fontSize: 12,
                    color: colors.onSurfaceVariant.withValues(alpha: .65),
                  ),
                ),
              ),
            ],
            const Spacer(),
            if (upstream != null) SyncPill(sync!),
          ],
        ),
      ),
    );
  }
}

/// Git page header: branch row and fetch, pull and push actions.
class BranchCard extends StatelessWidget {
  const BranchCard({
    super.key,
    required this.branch,
    required this.sync,
    required this.onBranch,
    required this.onAction,
  });

  final String branch;
  final BranchSync? sync;
  final VoidCallback? onBranch;

  /// Receives `fetch`, `pull` or `push`; null disables the actions.
  final ValueChanged<String>? onAction;

  @override
  Widget build(BuildContext context) {
    final ahead = sync?.ahead ?? 0;
    return Surface(
      padding: const EdgeInsets.fromLTRB(12, 14, 12, 12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          BranchHeader(branch: branch, sync: sync, onTap: onBranch),
          Row(
            spacing: 8,
            children: [
              for (final (action, label) in [
                ('fetch', 'gitFetch'),
                ('pull', 'gitPull'),
                ('push', 'gitPush'),
              ])
                Expanded(
                  child: _ActionButton(
                    label: context.tr(label),
                    badge: action == 'push' && ahead > 0 ? ahead : null,
                    onPressed: onAction == null
                        ? null
                        : () => onAction!(action),
                  ),
                ),
            ],
          ),
        ],
      ),
    );
  }
}

class _ActionButton extends StatelessWidget {
  const _ActionButton({
    required this.label,
    required this.onPressed,
    this.badge,
  });

  final String label;
  final VoidCallback? onPressed;
  final int? badge;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return TextButton(
      style: TextButton.styleFrom(
        backgroundColor: innerFill(context),
        disabledBackgroundColor: innerFill(context),
        foregroundColor: colors.onSurface,
        minimumSize: const Size(0, 40),
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(14)),
      ),
      onPressed: onPressed,
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Flexible(
            child: Text(label, maxLines: 1, overflow: TextOverflow.ellipsis),
          ),
          if (badge != null) ...[
            const SizedBox(width: 6),
            Container(
              padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
              decoration: BoxDecoration(
                color: colors.surfaceContainerHigh,
                borderRadius: BorderRadius.circular(999),
              ),
              child: Text(
                '$badge',
                style: const TextStyle(fontSize: 11, fontFeatures: _tabular),
              ),
            ),
          ],
        ],
      ),
    );
  }
}
