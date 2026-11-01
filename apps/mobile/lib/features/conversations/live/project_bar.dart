import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../ui/kit.dart';
import '../../../ui/project_icon.dart';

/// Search-field styled project switcher above the task filters.
class ProjectBar extends StatelessWidget {
  const ProjectBar({
    super.key,
    required this.project,
    required this.onTap,
    this.detail,
    this.title,
    this.leading,
    this.status,
  });

  /// Null selects every project.
  final Map<String, dynamic>? project;

  /// Null shows a fixed scope without the chevron.
  final VoidCallback? onTap;

  /// Secondary scope label, such as the selected host.
  final String? detail;

  /// Overrides the project name, such as with a host name.
  final String? title;

  /// Replaces the project icon inside the round badge.
  final Widget? leading;

  /// Shown before the chevron, such as a connection dot.
  final Widget? status;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final name =
        title ?? project?['name'] as String? ?? context.tr('allProjects');
    final heading = Text(
      name,
      maxLines: 1,
      overflow: TextOverflow.ellipsis,
      style: TextStyle(fontSize: 15, color: colors.onSurface),
    );
    return Semantics(
      button: onTap != null,
      label: detail == null ? name : '$name, $detail',
      excludeSemantics: true,
      child: Material(
        color: colors.surfaceContainerHigh,
        shape: const StadiumBorder(),
        clipBehavior: Clip.antiAlias,
        child: InkWell(
          onTap: onTap,
          child: SizedBox(
            height: 42,
            child: Padding(
              padding: const EdgeInsets.fromLTRB(8, 0, 14, 0),
              child: Row(
                children: [
                  Container(
                    width: 26,
                    height: 26,
                    decoration: BoxDecoration(
                      color: colors.surface,
                      shape: BoxShape.circle,
                    ),
                    alignment: Alignment.center,
                    child:
                        leading ??
                        (project == null
                            ? const AppIcon('folder', size: 15)
                            : ProjectIcon(project: project!, size: 15)),
                  ),
                  const SizedBox(width: 10),
                  if (detail == null)
                    Expanded(child: heading)
                  else ...[
                    Flexible(child: heading),
                    const SizedBox(width: 10),
                    Expanded(
                      child: Text(
                        detail!,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                          fontSize: 15,
                          color: colors.onSurfaceVariant.withValues(alpha: .65),
                        ),
                      ),
                    ),
                  ],
                  if (status != null) ...[const SizedBox(width: 8), status!],
                  if (onTap != null) ...[
                    const SizedBox(width: 8),
                    AppIcon('down', color: colors.onSurfaceVariant),
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
