import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../ui/kit.dart';
import '../../../ui/project_icon.dart';
import '../../../ui/theme.dart';

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
    this.onSearch,
    this.searching = false,
    this.onQuery,
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

  /// Replaces the chevron with a search button that toggles [searching].
  final VoidCallback? onSearch;

  /// Shows a search field in the same bar instead of the project.
  final bool searching;
  final ValueChanged<String>? onQuery;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final name =
        title ?? project?['name'] as String? ?? context.tr('allProjects');
    final heading = Text(
      name,
      maxLines: 1,
      overflow: TextOverflow.ellipsis,
      style: TextStyle(fontSize: TypeScale.body, color: colors.onSurface),
    );
    final search = onSearch == null
        ? null
        : IconButton(
            key: const ValueKey('project-bar-search'),
            onPressed: onSearch,
            tooltip: context.tr('searchTasks'),
            visualDensity: VisualDensity.compact,
            icon: AppIcon(
              searching ? 'close' : 'search',
              color: colors.onSurfaceVariant,
            ),
          );
    if (searching) {
      return Material(
        color: colors.surfaceContainerHigh,
        shape: const StadiumBorder(),
        clipBehavior: Clip.antiAlias,
        child: SizedBox(
          height: 42,
          child: Padding(
            padding: const EdgeInsets.fromLTRB(8, 0, 4, 0),
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
                  child: const AppIcon('search', size: 15),
                ),
                const SizedBox(width: 10),
                Expanded(
                  child: TextField(
                    autofocus: true,
                    onChanged: onQuery,
                    style: TextStyle(
                      fontSize: TypeScale.body,
                      color: colors.onSurface,
                    ),
                    // The bar is the field; the theme's outline and fill
                    // would draw a second box inside it.
                    decoration: InputDecoration(
                      isCollapsed: true,
                      filled: false,
                      border: InputBorder.none,
                      enabledBorder: InputBorder.none,
                      focusedBorder: InputBorder.none,
                      contentPadding: EdgeInsets.zero,
                      hintText: context.tr('searchTasks'),
                      hintStyle: TextStyle(
                        fontSize: TypeScale.body,
                        color: colors.onSurfaceVariant,
                      ),
                    ),
                  ),
                ),
                ?search,
              ],
            ),
          ),
        ),
      );
    }
    return Semantics(
      button: onTap != null,
      label: detail == null ? name : '$name, $detail',
      excludeSemantics: search == null,
      child: Material(
        color: colors.surfaceContainerHigh,
        shape: const StadiumBorder(),
        clipBehavior: Clip.antiAlias,
        child: InkWell(
          onTap: onTap,
          child: SizedBox(
            height: 42,
            child: Padding(
              padding: EdgeInsets.fromLTRB(8, 0, search == null ? 14 : 4, 0),
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
                  Expanded(
                    child: Row(
                      children: [
                        Flexible(child: heading),
                        if (detail != null) ...[
                          const SizedBox(width: 10),
                          Flexible(
                            child: Text(
                              detail!,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: TextStyle(
                                fontSize: TypeScale.body,
                                color: colors.onSurfaceVariant.withValues(
                                  alpha: .65,
                                ),
                              ),
                            ),
                          ),
                        ],
                      ],
                    ),
                  ),
                  if (status != null) ...[const SizedBox(width: 8), status!],
                  if (search != null)
                    search
                  else if (onTap != null) ...[
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
