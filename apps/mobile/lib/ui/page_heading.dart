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
    this.subtitle,
    this.accessory,
  });
  final String title;
  final bool enabled;
  final double size;

  /// Secondary line under the title, such as a file's directory.
  final Widget? subtitle;

  /// Small marker after the title, such as an unsaved-changes dot.
  final Widget? accessory;

  @override
  Widget build(BuildContext context) {
    Widget label = Text(
      title,
      maxLines: 1,
      overflow: TextOverflow.ellipsis,
      style: Theme.of(context).textTheme.titleLarge?.copyWith(fontSize: size),
    );
    if (accessory != null) {
      label = Row(
        children: [
          Flexible(child: label),
          const SizedBox(width: 8),
          accessory!,
        ],
      );
    }
    if (subtitle != null) {
      label = Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: [label, subtitle!],
      );
    }
    if (ModalRoute.canPopOf(context) != true) return label;
    return Tooltip(
      message: context.tr('back'),
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
