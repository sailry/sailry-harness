import 'dart:async';

import 'package:flutter/foundation.dart' show ValueListenable;
import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../ui/kit.dart';
import '../../../ui/toast.dart';
import '../../resources/session_services.dart';
import '../../terminal/terminal_page.dart';
import 'presentation.dart';
import 'progress.dart';
import 'status_chips.dart';
import 'status_feed.dart';
import 'terminal_tasks.dart';
import 'tool_row.dart';
import 'turn_frame.dart';

/// Core activity still running for a conversation.
class _Activity {
  _Activity(
    HostConnection host,
    Map<String, dynamic> session,
    Map<String, dynamic> view,
    SessionServices services,
  ) {
    final page = object(object(view['snapshot'])['page']);
    final calls = objects(view['calls']);
    final run = objects(page['runs']).where(activeRun).lastOrNull;
    final latest = run == null ? null : turnProgress(calls, run['turn']);
    if (latest != null) {
      final (done, total) = progressCounts(latest);
      if (done < total) progress = latest;
    }
    final records = objects(page['children']);
    for (final value in object(object(view['children'])['sessions']).values) {
      final reference = object(value);
      if (reference['index'] case final int index when index < records.length) {
        if (activeRun(object(records[index]['run']))) {
          children.add((records[index], reference));
        }
      }
    }
    background.addAll(
      objects(page['background']).where(
        (task) =>
            task['state'] == 'running' &&
            object(task['kind'])['kind'] != 'agent',
      ),
    );
    terminals.addAll(
      objects(host.snapshot['terminals']).where(
        (terminal) =>
            terminal['worktree'] == session['worktree'] &&
            terminalStatus(terminal) == 'running',
      ),
    );
    ports.addAll(services.sources(running: true));
  }

  Map<String, dynamic>? progress;
  final children = <(Map<String, dynamic>, Map<String, dynamic>)>[];
  final background = <Map<String, dynamic>>[];
  final terminals = <Map<String, dynamic>>[];
  final ports = <Map<String, dynamic>>[];

  bool get isEmpty =>
      progress == null &&
      children.isEmpty &&
      background.isEmpty &&
      terminals.isEmpty &&
      ports.isEmpty;
}

/// One row of chips above the composer; each opens the grouped activity sheet.
class ActivityStrip extends StatefulWidget {
  const ActivityStrip({
    super.key,
    required this.host,
    required this.session,
    required this.updates,
    this.onOpenChild,
  });

  final HostConnection host;
  final Map<String, dynamic> session;
  final ValueListenable<Map<String, dynamic>> updates;
  final ValueChanged<Map<String, dynamic>>? onOpenChild;

  @override
  State<ActivityStrip> createState() => _ActivityStripState();
}

class _ActivityStripState extends State<ActivityStrip> {
  late SessionServices _services = SessionServices(
    widget.host,
    widget.session['id'] as String,
  );
  late StatusFeed _status = StatusFeed(widget.host, widget.session);

  @override
  void initState() {
    super.initState();
    widget.host.addListener(_resume);
    unawaited(_services.watch());
    unawaited(_status.watch());
  }

  @override
  void didUpdateWidget(covariant ActivityStrip oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(oldWidget.host, widget.host)) {
      oldWidget.host.removeListener(_resume);
      widget.host.addListener(_resume);
    }
    if (!identical(_services.host, widget.host) ||
        _services.session != widget.session['id']) {
      _services.dispose();
      _services = SessionServices(widget.host, widget.session['id'] as String);
      unawaited(_services.watch());
    }
    final status = _status.rebind(widget.host, widget.session);
    if (!identical(status, _status)) {
      _status = status;
      unawaited(_status.watch());
    }
  }

  // A failed subscription resumes once the host is connected again.
  void _resume() {
    if (widget.host.connected) {
      unawaited(_services.watch());
      unawaited(_status.watch());
    }
  }

  @override
  void dispose() {
    widget.host.removeListener(_resume);
    _services.dispose();
    _status.dispose();
    super.dispose();
  }

  Listenable get _changes =>
      Listenable.merge([widget.host, widget.updates, _services, _status]);

  _Activity _activity() =>
      _Activity(widget.host, widget.session, widget.updates.value, _services);

  void _open() => showAppSheet(
    context,
    context.tr('tasks'),
    child: ListenableBuilder(
      listenable: _changes,
      builder: (_, _) => _ActivitySheet(
        activity: _activity(),
        host: widget.host,
        page: context,
        onOpenChild: widget.onOpenChild,
      ),
    ),
  );

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: _changes,
    builder: (context, _) {
      final activity = _activity();
      if (activity.isEmpty && _status.entries.isEmpty) {
        return const SizedBox.shrink();
      }
      final colors = Theme.of(context).colorScheme;
      final progress = activity.progress;
      final ports = activity.ports;
      return Padding(
        padding: const EdgeInsets.fromLTRB(12, 0, 12, 2),
        child: SingleChildScrollView(
          scrollDirection: Axis.horizontal,
          child: Row(
            spacing: 8,
            children: [
              if (_status.entries.isNotEmpty) StatusChips(feed: _status),
              if (progress != null)
                _Chip(
                  label: context.tr('activityProgress'),
                  compact: true,
                  leading: SizedBox.square(
                    dimension: 15,
                    child: CircularProgressIndicator(
                      value:
                          progressCounts(progress).$1 /
                          progressCounts(progress).$2,
                      strokeWidth: 2.5,
                      color: colors.onSurface,
                      backgroundColor: colors.outlineVariant,
                    ),
                  ),
                  value:
                      '${progressCounts(progress).$1}/${progressCounts(progress).$2}',
                  onPressed: _open,
                ),
              if (activity.children.isNotEmpty)
                _Chip(
                  icon: 'spark',
                  running: true,
                  label: context.tr('conversationChild'),
                  value: '${activity.children.length}',
                  onPressed: _open,
                ),
              if (activity.background.isNotEmpty)
                _Chip(
                  icon: 'clock',
                  running: true,
                  label: context.tr('activityBackground'),
                  value: '${activity.background.length}',
                  onPressed: _open,
                ),
              if (activity.terminals.isNotEmpty)
                _Chip(
                  icon: 'terminal',
                  running: true,
                  label: context.tr('terminal'),
                  value: '${activity.terminals.length}',
                  onPressed: _open,
                ),
              if (ports.isNotEmpty)
                _Chip(
                  icon: 'globe',
                  label: context.tr('activityPorts'),
                  compact: true,
                  value: [
                    ':${object(ports.first['service'])['port']}',
                    if (ports.length > 1) '+${ports.length - 1}',
                  ].join(' '),
                  onPressed: _open,
                ),
            ],
          ),
        ),
      );
    },
  );
}

class _Chip extends StatelessWidget {
  const _Chip({
    required this.label,
    required this.value,
    required this.onPressed,
    this.icon,
    this.leading,
    this.running = false,
    this.compact = false,
  });

  /// Compact chips show only their value; the label stays accessible.
  final bool compact;
  final String label;
  final String value;
  final VoidCallback onPressed;
  final String? icon;
  final Widget? leading;
  final bool running;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final chip = OutlinedButton(
      onPressed: onPressed,
      style: OutlinedButton.styleFrom(
        foregroundColor: colors.onSurfaceVariant,
        side: BorderSide(color: colors.outlineVariant),
        shape: const StadiumBorder(),
        padding: const EdgeInsets.symmetric(horizontal: 11),
        minimumSize: const Size(0, 30),
        fixedSize: const Size.fromHeight(30),
        tapTargetSize: MaterialTapTargetSize.shrinkWrap,
        visualDensity: VisualDensity.compact,
        textStyle: const TextStyle(fontSize: transcriptFontSize),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        spacing: 6,
        children: [
          leading ??
              AppIcon(
                icon!,
                size: 15,
                color: running ? colors.onSurface : colors.onSurfaceVariant,
              ),
          if (!compact) Text(label),
          Text(value, style: TextStyle(color: colors.onSurface)),
        ],
      ),
    );
    return compact ? Semantics(label: label, child: chip) : chip;
  }
}

class _ActivitySheet extends StatelessWidget {
  const _ActivitySheet({
    required this.activity,
    required this.host,
    required this.page,
    this.onOpenChild,
  });

  final _Activity activity;
  final HostConnection host;

  /// The conversation page, which outlives this sheet for navigation.
  final BuildContext page;
  final ValueChanged<Map<String, dynamic>>? onOpenChild;

  Future<void> _stop(
    BuildContext context,
    Map<String, dynamic> terminal,
  ) async {
    try {
      await host.command('close_terminal', {
        'worktree': terminal['worktree'],
        'terminal': terminal['id'],
      });
    } catch (error) {
      if (context.mounted) {
        showToast(context, failureLabel(error, translate: context.tr));
      }
    }
  }

  Future<void> _service(Map<String, dynamic> source) async {
    try {
      await openService(page, host, source);
    } catch (error) {
      if (page.mounted) {
        showToast(page, failureLabel(error, translate: page.tr));
      }
    }
  }

  // Leaves the sheet before navigating from the conversation page.
  void _leave(BuildContext context, VoidCallback action) {
    Navigator.pop(context);
    action();
  }

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final progress = activity.progress;
    Widget group(String key) => Padding(
      padding: const EdgeInsets.only(top: 12, bottom: 2),
      child: Text(
        context.tr(key),
        style: TextStyle(
          fontSize: transcriptFontSize,
          color: colors.onSurfaceVariant.withValues(alpha: .65),
        ),
      ),
    );
    Widget action(String key, VoidCallback? onPressed, {Color? color}) =>
        TextButton(
          onPressed: onPressed,
          style: TextButton.styleFrom(
            foregroundColor: color ?? colors.onSurface,
            minimumSize: const Size(0, 32),
            padding: const EdgeInsets.symmetric(horizontal: 8),
            tapTargetSize: MaterialTapTargetSize.shrinkWrap,
            visualDensity: VisualDensity.compact,
          ),
          child: Text(context.tr(key)),
        );
    if (activity.isEmpty) {
      return Padding(
        padding: const EdgeInsets.symmetric(vertical: 24),
        child: EmptyState(icon: 'task', message: context.tr('resourceEmpty')),
      );
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (progress != null) ...[
          group('activityProgress'),
          TaskProgress(progress: progress),
        ],
        if (activity.children.isNotEmpty) ...[
          group('conversationChild'),
          for (final (child, reference) in activity.children)
            ToolRow(
              icon: 'spark',
              label: text(child['name'], context.tr('conversationChild')),
              running: true,
              action: AppIcon(
                'chevron',
                size: 12,
                color: colors.onSurfaceVariant,
              ),
              onTap: onOpenChild == null
                  ? null
                  : () => _leave(context, () => onOpenChild!(reference)),
            ),
        ],
        if (activity.background.isNotEmpty) ...[
          group('activityBackground'),
          for (final task in activity.background)
            ToolRow(icon: 'clock', label: text(task['title']), running: true),
        ],
        if (activity.terminals.isNotEmpty) ...[
          group('terminal'),
          for (final terminal in activity.terminals)
            ToolRow(
              icon: 'terminal',
              label: terminalTitle(terminal, translate: context.tr),
              running: true,
              onTap: () => _leave(
                context,
                () => pushPage(
                  page,
                  TerminalPage(
                    hostId: host.id,
                    terminalId: text(terminal['id']),
                    worktreeId: terminal['worktree'] as String?,
                    title: terminalTitle(terminal, translate: context.tr),
                  ),
                ),
              ),
              action: action(
                'terminalStop',
                host.connected ? () => _stop(context, terminal) : null,
                color: colors.error,
              ),
            ),
        ],
        if (activity.ports.isNotEmpty) ...[
          group('activityPorts'),
          for (final source in activity.ports)
            ToolRow(
              icon: 'globe',
              label: '${object(source['service'])['port']}',
              detail: text(object(source['service'])['url']),
              action: action(
                'resourceServiceOpen',
                host.connected
                    ? () => _leave(context, () => _service(source))
                    : null,
              ),
            ),
        ],
      ],
    );
  }
}
