import 'package:flutter/material.dart';

import '../l10n/strings.dart';
import 'icons.dart';

/// Route-owned navigation preserves native back handling and PopScope guards.
class PageHeading extends StatelessWidget {
  const PageHeading({
    super.key,
    required this.title,
    this.enabled = true,
    this.size = 22,
  });
  final String title;
  final bool enabled;
  final double size;

  @override
  Widget build(BuildContext context) {
    final label = Text(
      title,
      maxLines: 1,
      overflow: TextOverflow.ellipsis,
      style: Theme.of(context).textTheme.titleLarge?.copyWith(fontSize: size),
    );
    if (ModalRoute.canPopOf(context) != true) return label;
    return Tooltip(
      message: tr('back'),
      child: InkWell(
        borderRadius: BorderRadius.circular(10),
        onTap: enabled ? () => Navigator.maybePop(context) : null,
        child: ConstrainedBox(
          constraints: const BoxConstraints(minHeight: 36),
          child: Row(
            children: [
              const AppIcon('back', size: 16),
              const SizedBox(width: 8),
              Expanded(child: label),
            ],
          ),
        ),
      ),
    );
  }
}
