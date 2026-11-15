import 'package:flutter/material.dart';

/// Rows grouped on one rounded card under an optional small heading, with
/// hairlines between them inset to the text. Settings and sheets use it
/// where related choices belong together.
class ListGroup extends StatelessWidget {
  const ListGroup({super.key, this.title, required this.children});

  final String? title;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (title case final title?)
          Padding(
            padding: const EdgeInsets.fromLTRB(14, 0, 14, 8),
            child: Semantics(
              header: true,
              child: Text(
                title,
                style: TextStyle(fontSize: 13, color: colors.onSurfaceVariant),
              ),
            ),
          ),
        Material(
          color: colors.surfaceContainer,
          borderRadius: BorderRadius.circular(16),
          clipBehavior: Clip.antiAlias,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              for (final (index, child) in children.indexed) ...[
                if (index > 0)
                  Divider(
                    height: 1,
                    thickness: .5,
                    indent: 14,
                    endIndent: 14,
                    color: colors.outlineVariant,
                  ),
                child,
              ],
            ],
          ),
        ),
      ],
    );
  }
}

/// A row of a [ListGroup]: a title, an optional muted line under it and a
/// trailing mark or value.
class ListGroupRow extends StatelessWidget {
  const ListGroupRow({
    super.key,
    required this.title,
    this.subtitle,
    this.trailing,
    this.color,
    this.selected,
    this.onTap,
  });

  final String title;
  final String? subtitle;
  final Widget? trailing;

  /// The title's color; the surface's text color when null.
  final Color? color;

  /// Marks a single-choice row for assistive technology.
  final bool? selected;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Semantics(
      selected: selected,
      button: onTap != null,
      enabled: onTap != null,
      child: InkWell(
        onTap: onTap,
        child: ConstrainedBox(
          constraints: const BoxConstraints(minHeight: 48),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 11),
            child: Opacity(
              opacity: onTap == null ? .5 : 1,
              child: Row(
                children: [
                  Expanded(
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      crossAxisAlignment: CrossAxisAlignment.start,
                      spacing: 3,
                      children: [
                        Text(
                          title,
                          style: TextStyle(
                            fontSize: 15,
                            color: color ?? colors.onSurface,
                          ),
                        ),
                        if (subtitle case final subtitle?)
                          Text(
                            subtitle,
                            style: TextStyle(
                              fontSize: 12.5,
                              height: 1.3,
                              color: colors.onSurfaceVariant,
                            ),
                          ),
                      ],
                    ),
                  ),
                  if (trailing case final trailing?) ...[
                    const SizedBox(width: 12),
                    trailing,
                  ],
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}
