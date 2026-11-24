import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../runtime/session.dart';
import '../../ui/kit.dart';
import '../../ui/theme.dart';
import 'host/host_info_card.dart' show StatusDot, statusDotColor;
import 'host/metric_history.dart';

/// Icon size for a [ScopeRow]'s leading mark.
const scopeIconSize = 17.0;

/// A host's route tone: blue direct, amber relayed, muted offline.
Color hostTone(BuildContext context, HostConnection host) {
  if (!host.connected) return Theme.of(context).colorScheme.onSurfaceVariant;
  return linkRoute(host.transport) == 'relay'
      ? SailryTheme.statusColor(context, StatusTone.warning)
      : SailryTheme.accentColor(context, AccentTone.blue);
}

/// One row of a host, project or workspace picker: a leading mark in a round
/// well, a title with an optional status mark, a muted detail line and a
/// trailing control. The chosen row stands on the brand's tint with a check;
/// a row without [onTap] is disabled.
class ScopeRow extends StatelessWidget {
  const ScopeRow({
    super.key,
    required this.leading,
    required this.title,
    this.status,
    this.subtitle,
    this.trailing,
    this.selected = false,
    this.nested = false,
    this.muted = false,
    this.titleLines = 1,
    this.tapKey,
    this.onTap,
  });

  final Widget leading;
  final String title;

  /// A small mark after the title, such as a host's connection dot.
  final Widget? status;
  final String? subtitle;

  /// Replaces the selection check.
  final Widget? trailing;
  final bool selected;

  /// Indents a row that belongs to a section header.
  final bool nested;

  /// Shows the title in the muted color, such as for an offline host.
  final bool muted;
  final int titleLines;

  /// Identifies the row's tap target.
  final Key? tapKey;
  final VoidCallback? onTap;

  bool get enabled => onTap != null;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final brand = SailryTheme.brand(context);
    final radius = BorderRadius.circular(12);
    final mark =
        trailing ??
        (selected ? AppIcon('check', size: 16, color: brand.color) : null);
    return Padding(
      padding: EdgeInsets.only(left: nested ? 20 : 0),
      child: Semantics(
        selected: selected,
        child: Ink(
          decoration: BoxDecoration(
            color: selected ? brand.selection : null,
            borderRadius: radius,
          ),
          child: InkWell(
            key: tapKey,
            borderRadius: radius,
            onTap: onTap,
            child: ConstrainedBox(
              constraints: const BoxConstraints(minHeight: 50),
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 8),
                child: Opacity(
                  opacity: enabled ? 1 : .5,
                  child: Row(
                    children: [
                      Container(
                        width: 32,
                        height: 32,
                        decoration: BoxDecoration(
                          color: colors.surfaceContainerHigh,
                          shape: BoxShape.circle,
                        ),
                        alignment: Alignment.center,
                        child: leading,
                      ),
                      const SizedBox(width: 12),
                      Expanded(
                        child: Column(
                          mainAxisSize: MainAxisSize.min,
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Row(
                              children: [
                                Flexible(
                                  child: Text(
                                    title,
                                    maxLines: titleLines,
                                    overflow: TextOverflow.ellipsis,
                                    style: TextStyle(
                                      fontSize: 14,
                                      fontWeight: selected
                                          ? FontWeight.w500
                                          : null,
                                      color: selected
                                          ? brand.color
                                          : muted
                                          ? colors.onSurfaceVariant
                                          : colors.onSurface,
                                    ),
                                  ),
                                ),
                                if (status case final status?) ...[
                                  const SizedBox(width: 7),
                                  status,
                                ],
                              ],
                            ),
                            if (subtitle case final subtitle?) ...[
                              const SizedBox(height: 2),
                              Text(
                                subtitle,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: TextStyle(
                                  fontSize: 12,
                                  height: 1.3,
                                  color: colors.onSurfaceVariant,
                                ),
                              ),
                            ],
                          ],
                        ),
                      ),
                      if (mark != null) ...[const SizedBox(width: 8), mark],
                    ],
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// Host row with its route icon, status dot and link detail. A host list
/// places an action in [trailing]; a section header passes [expanded] for its
/// disclosure chevron.
class HostRow extends StatelessWidget {
  const HostRow({
    super.key,
    required this.host,
    required this.onTap,
    this.trailing,
    this.selected = false,
    this.expanded,
  });
  final HostConnection host;
  final VoidCallback? onTap;
  final Widget? trailing;
  final bool selected;
  final bool? expanded;

  @override
  Widget build(BuildContext context) {
    final status = hostStatus(host.connected, host.error);
    final route = linkRoute(host.transport);
    final latency = linkLatency(host.transport);
    final connection = switch (status) {
      HostStatus.online => [
        if (route != null)
          context.tr(
            route == 'p2p' ? 'connectionRouteP2p' : 'connectionRouteRelay',
          ),
        if (latency != null) '${latency.round()} ms',
      ].join('  '),
      HostStatus.connecting => context.tr('hostConnecting'),
      HostStatus.offline => context.tr('offline'),
    };
    final origin = host.originDescription(
      context.tr,
      AppSession.maybeOf(context)?.hosts ?? [host],
    );
    final detail = [
      if (origin.isNotEmpty) origin,
      if (connection.isNotEmpty) connection,
    ].join(' · ');
    final open = expanded;
    return Semantics(
      expanded: open,
      child: ScopeRow(
        tapKey: ValueKey('host-row-${host.id}'),
        leading: AppIcon(
          'server',
          size: scopeIconSize,
          color: hostTone(context, host),
        ),
        title: host.label,
        muted: !host.connected,
        status: StatusDot(color: statusDotColor(context, status)),
        subtitle: detail.isEmpty ? null : detail,
        selected: selected,
        trailing:
            trailing ??
            (open == null
                ? null
                : AnimatedRotation(
                    turns: open ? .5 : 0,
                    duration: MediaQuery.disableAnimationsOf(context)
                        ? Duration.zero
                        : kThemeAnimationDuration,
                    child: const AppIcon('down'),
                  )),
        onTap: onTap,
      ),
    );
  }
}

/// A host and the rows chosen under it. [expanded] opens the section at
/// first, such as when it holds the current choice.
typedef ScopeSection = ({
  HostConnection host,
  List<Widget> children,
  bool expanded,
});

/// The shared host, project and workspace picker: fixed [rows] first, then
/// one collapsible section per host. A lone section starts open; the user's
/// toggles persist while the sheet follows later host updates.
class ScopePicker extends StatefulWidget {
  const ScopePicker({
    super.key,
    this.rows = const [],
    this.sections = const [],
  });

  final List<Widget> rows;
  final List<ScopeSection> sections;

  @override
  State<ScopePicker> createState() => _ScopePickerState();
}

class _ScopePickerState extends State<ScopePicker> {
  final _toggled = <String, bool>{};

  @override
  Widget build(BuildContext context) {
    final sections = widget.sections;
    final duration = MediaQuery.disableAnimationsOf(context)
        ? Duration.zero
        : kThemeAnimationDuration;
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        ...widget.rows,
        if (widget.rows.isNotEmpty && sections.isNotEmpty)
          const Divider(height: 12),
        for (final section in sections)
          Builder(
            key: ValueKey(section.host.id),
            builder: (context) {
              final id = section.host.id;
              final open =
                  _toggled[id] ?? (section.expanded || sections.length == 1);
              return Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  HostRow(
                    host: section.host,
                    expanded: open,
                    onTap: () => setState(() => _toggled[id] = !open),
                  ),
                  ClipRect(
                    child: AnimatedSize(
                      duration: duration,
                      curve: Curves.easeOutCubic,
                      alignment: Alignment.topCenter,
                      child: open
                          ? Column(
                              mainAxisSize: MainAxisSize.min,
                              crossAxisAlignment: CrossAxisAlignment.stretch,
                              children: section.children,
                            )
                          : const SizedBox(width: double.infinity),
                    ),
                  ),
                ],
              );
            },
          ),
      ],
    );
  }
}

/// Picks one of the session's hosts after optional fixed [rows]; returns the
/// chosen host's ID.
Future<String?> showHostPicker(
  BuildContext context,
  AppSession session, {
  required String? selected,
  List<Widget> rows = const [],
}) => showAppSheet<String>(
  context,
  context.tr('selectHost'),
  child: ListenableBuilder(
    listenable: session,
    builder: (context, _) => ScopePicker(
      rows: [
        ...rows,
        for (final host in session.hosts)
          HostRow(
            host: host,
            selected: host.id == selected,
            onTap: () => Navigator.pop(context, host.id),
          ),
      ],
    ),
  ),
);
