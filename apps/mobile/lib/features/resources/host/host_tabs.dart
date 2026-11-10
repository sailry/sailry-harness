import 'dart:ui';

import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/kit.dart';
import '../../../ui/project_icon.dart';
import '../../../ui/theme.dart';
import 'host_parts.dart';
import 'metric_charts.dart';
import 'metric_history.dart';

enum HostTab { projects, processes }

/// Pinned glass tab row with the active tab's sort control.
class HostTabsHeader extends SliverPersistentHeaderDelegate {
  const HostTabsHeader({
    required this.tab,
    required this.projects,
    required this.processes,
    required this.byMemory,
    required this.onTab,
    required this.onSort,
  });

  final HostTab tab;
  final int projects;
  final int processes;
  final bool byMemory;
  final ValueChanged<HostTab> onTab;
  final ValueChanged<bool> onSort;

  static const _height = 48.0;

  @override
  double get minExtent => _height;
  @override
  double get maxExtent => _height;

  @override
  bool shouldRebuild(HostTabsHeader old) => true;

  @override
  Widget build(BuildContext context, double shrinkOffset, bool overlaps) {
    final colors = Theme.of(context).colorScheme;
    Widget button(HostTab value, String label, int count) {
      final selected = tab == value;
      return Semantics(
        selected: selected,
        child: TextButton(
          key: ValueKey('host-tab-${value.name}'),
          style: TextButton.styleFrom(
            padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 7),
            minimumSize: const Size(0, 34),
            tapTargetSize: MaterialTapTargetSize.shrinkWrap,
            backgroundColor: selected
                ? colors.surfaceContainerHigh
                : Colors.transparent,
            foregroundColor: selected
                ? colors.onSurface
                : colors.onSurfaceVariant,
            shape: const StadiumBorder(),
          ),
          onPressed: () => onTab(value),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.baseline,
            textBaseline: TextBaseline.alphabetic,
            children: [
              Text(
                label,
                style: TextStyle(
                  fontSize: 14,
                  fontWeight: selected ? FontWeight.w500 : FontWeight.w400,
                ),
              ),
              const SizedBox(width: 6),
              Text('$count', style: dimStyle(context)),
            ],
          ),
        ),
      );
    }

    // The header must fill its declared extent, or the sliver geometry is
    // invalid and the lists below it never lay out.
    return SizedBox.expand(
      child: ClipRect(
        child: BackdropFilter(
          filter: ImageFilter.blur(sigmaX: 12, sigmaY: 12),
          child: ColoredBox(
            color: overlaps
                ? colors.surface.withValues(alpha: .86)
                : Colors.transparent,
            child: Row(
              children: [
                button(HostTab.projects, context.tr('project'), projects),
                const SizedBox(width: 4),
                button(
                  HostTab.processes,
                  context.tr('hostProcesses'),
                  processes,
                ),
                const Spacer(),
                if (tab == HostTab.processes)
                  _SortMenu(byMemory: byMemory, onSort: onSort)
                else
                  Text(
                    context.tr('hostSortRecent'),
                    style: mutedStyle(context),
                  ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _SortMenu extends StatelessWidget {
  const _SortMenu({required this.byMemory, required this.onSort});
  final bool byMemory;
  final ValueChanged<bool> onSort;

  @override
  Widget build(BuildContext context) => MenuAnchor(
    style: const MenuStyle(
      backgroundColor: WidgetStatePropertyAll(Colors.transparent),
      surfaceTintColor: WidgetStatePropertyAll(Colors.transparent),
      elevation: WidgetStatePropertyAll(0),
      padding: WidgetStatePropertyAll(EdgeInsets.zero),
    ),
    alignmentOffset: const Offset(-96, 4),
    menuChildren: [
      Surface(
        kind: SurfaceKind.sheet,
        radius: 16,
        padding: const EdgeInsets.all(6),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            for (final (memory, key) in [
              (false, 'hostSortCpu'),
              (true, 'hostSortMemory'),
            ])
              Semantics(
                selected: memory == byMemory,
                child: MenuItemButton(
                  trailingIcon: memory == byMemory
                      ? const AppIcon('check', size: 16)
                      : const SizedBox(width: 16),
                  onPressed: () => onSort(memory),
                  child: SizedBox(width: 96, child: Text(context.tr(key))),
                ),
              ),
          ],
        ),
      ),
    ],
    builder: (context, controller, _) => TextButton(
      key: const ValueKey('host-process-sort'),
      style: TextButton.styleFrom(
        foregroundColor: Theme.of(context).colorScheme.onSurfaceVariant,
        padding: const EdgeInsets.symmetric(horizontal: 6),
        minimumSize: const Size(0, 34),
        tapTargetSize: MaterialTapTargetSize.shrinkWrap,
      ),
      onPressed: () =>
          controller.isOpen ? controller.close() : controller.open(),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(
            context.tr(byMemory ? 'hostSortMemory' : 'hostSortCpu'),
            style: const TextStyle(fontSize: 12),
          ),
          const SizedBox(width: 3),
          const AppIcon('down', size: 12),
        ],
      ),
    ),
  );
}

class _SearchField extends StatelessWidget {
  const _SearchField({required this.hint, required this.onChanged});
  final String hint;
  final ValueChanged<String> onChanged;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.only(top: 6, bottom: 2),
    child: TextField(
      decoration: InputDecoration(
        hintText: hint,
        isDense: true,
        prefixIcon: const Padding(
          padding: EdgeInsets.all(10),
          child: AppIcon('search', size: 16),
        ),
        prefixIconConstraints: const BoxConstraints(minWidth: 36),
      ),
      onChanged: onChanged,
    ),
  );
}

Widget _rowDivider(BuildContext context, bool visible) => SizedBox(
  height: 1,
  child: ColoredBox(
    color: visible
        ? Theme.of(context).colorScheme.outlineVariant
        : Colors.transparent,
  ),
);

/// Searchable top processes with a CPU-share line under each row.
class ProcessList extends StatelessWidget {
  const ProcessList({
    super.key,
    required this.processes,
    required this.query,
    required this.byMemory,
    required this.onQuery,
  });

  final List<Map<String, dynamic>> processes;
  final String query;
  final bool byMemory;
  final ValueChanged<String> onQuery;

  @override
  Widget build(BuildContext context) {
    if (processes.isEmpty) {
      return Padding(
        padding: const EdgeInsets.only(top: 24),
        child: EmptyState(
          icon: 'cpu',
          message: context.tr('hostProcessesEmpty'),
        ),
      );
    }
    final ranked = rankProcesses(
      processes,
      query: query,
      byMemory: byMemory,
      limit: processLimit + 1,
    );
    final visible = ranked.take(processLimit).toList();
    final normal = metricColor(context, MetricTone.cpu).withValues(alpha: .7);
    final busy = SailryTheme.statusColor(context, StatusTone.warning);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _SearchField(
          hint: context.tr('hostSearchProcesses'),
          onChanged: onQuery,
        ),
        if (visible.isEmpty)
          Padding(
            padding: const EdgeInsets.only(top: 24),
            child: EmptyState(
              icon: 'search',
              message: context.tr('hostNoMatches'),
            ),
          ),
        for (final (index, process) in visible.indexed) ...[
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 10),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.baseline,
              textBaseline: TextBaseline.alphabetic,
              children: [
                Expanded(
                  child: Text.rich(
                    TextSpan(
                      text: text(process['name']),
                      children: [
                        TextSpan(
                          text: '  ${process['pid'] ?? ''}',
                          style: dimStyle(context),
                        ),
                      ],
                    ),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(fontSize: 14),
                  ),
                ),
                SizedBox(
                  width: 56,
                  child: Text(
                    process['cpu_basis_points'] is num
                        ? basisPercent(process['cpu_basis_points'])
                        : '—',
                    textAlign: TextAlign.end,
                    style: const TextStyle(fontSize: 14, fontFeatures: figures),
                  ),
                ),
                SizedBox(
                  width: 64,
                  child: Text(
                    formatBytes(number(process['memory_bytes'])),
                    textAlign: TextAlign.end,
                    style: dimStyle(context),
                  ),
                ),
              ],
            ),
          ),
          Stack(
            children: [
              _rowDivider(context, index < visible.length - 1),
              Builder(
                builder: (context) {
                  final share = (number(process['cpu_basis_points']) / 10000)
                      .clamp(0, 1)
                      .toDouble();
                  return FractionallySizedBox(
                    alignment: AlignmentDirectional.centerStart,
                    widthFactor: share < .015 ? .015 : share,
                    child: Container(
                      height: 2,
                      decoration: BoxDecoration(
                        color: share >= .5 ? busy : normal,
                        borderRadius: BorderRadius.circular(1),
                      ),
                    ),
                  );
                },
              ),
            ],
          ),
        ],
        if (ranked.length > processLimit)
          Padding(
            padding: const EdgeInsets.only(top: 8),
            child: Text(
              context
                  .tr('hostProcessLimit')
                  .replaceAll('{count}', '$processLimit'),
              style: dimStyle(context),
            ),
          ),
      ],
    );
  }
}

/// Registered projects with their session counts and paths.
class ProjectList extends StatelessWidget {
  const ProjectList({
    super.key,
    required this.projects,
    required this.sessions,
    required this.query,
    required this.onQuery,
    required this.onOpen,
  });

  final List<Map<String, dynamic>> projects;
  final List<Map<String, dynamic>> sessions;
  final String query;
  final ValueChanged<String> onQuery;
  final ValueChanged<Map<String, dynamic>> onOpen;

  static const _searchThreshold = 8;

  @override
  Widget build(BuildContext context) {
    final needle = query.trim().toLowerCase();
    final ordered = recentProjects(projects, sessions)
        .where(
          (project) =>
              needle.isEmpty ||
              text(project['name']).toLowerCase().contains(needle) ||
              text(project['path']).toLowerCase().contains(needle),
        )
        .toList();
    final counts = <Object?, int>{};
    for (final session in sessions) {
      counts.update(session['project'], (n) => n + 1, ifAbsent: () => 1);
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (projects.length > _searchThreshold)
          _SearchField(
            hint: context.tr('hostSearchProjects'),
            onChanged: onQuery,
          ),
        if (ordered.isEmpty && projects.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(top: 24),
            child: EmptyState(
              icon: 'search',
              message: context.tr('hostNoMatches'),
            ),
          ),
        for (final (index, project) in ordered.indexed)
          InkWell(
            key: ValueKey('host-project-${project['id']}'),
            borderRadius: BorderRadius.circular(12),
            onTap: () => onOpen(project),
            child: Padding(
              padding: const EdgeInsets.only(top: 10),
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Container(
                    width: 36,
                    height: 36,
                    decoration: BoxDecoration(
                      color: SailryTheme.insetFill(context),
                      borderRadius: BorderRadius.circular(10),
                    ),
                    alignment: Alignment.center,
                    child: ProjectIcon(project: project),
                  ),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        Row(
                          children: [
                            Expanded(
                              child: Text(
                                text(project['name']),
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: const TextStyle(fontSize: 14),
                              ),
                            ),
                            const SizedBox(width: 8),
                            AppIcon('chat', size: 12, color: chartDim(context)),
                            const SizedBox(width: 4),
                            Text(
                              '${counts[project['id']] ?? 0}',
                              style: dimStyle(context),
                            ),
                          ],
                        ),
                        const SizedBox(height: 3),
                        Text(
                          text(project['path']),
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: dimStyle(
                            context,
                          ).copyWith(fontFamily: 'monospace'),
                        ),
                        const SizedBox(height: 10),
                        _rowDivider(context, index < ordered.length - 1),
                      ],
                    ),
                  ),
                ],
              ),
            ),
          ),
      ],
    );
  }
}
