import 'package:flutter/material.dart';

import '../../ui/kit.dart';
import '../../ui/theme.dart';

/// One entry row of [ResourceEntries].
typedef ResourceEntry = ({
  String icon,
  String label,
  String? detail,
  VoidCallback onTap,
});

/// Resource entry rows in one card.
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
                            style: const TextStyle(fontSize: TypeScale.body),
                          ),
                        ),
                        if (entry.detail != null) ...[
                          const SizedBox(width: 8),
                          Text(
                            entry.detail!,
                            style: TextStyle(
                              fontSize: TypeScale.secondary,
                              color: dim,
                            ),
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
