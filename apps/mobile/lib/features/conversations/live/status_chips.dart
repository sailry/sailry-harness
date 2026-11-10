import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/icons.dart' show catalogIcon, catalogIcons;
import '../../../ui/kit.dart';
import '../../../ui/theme.dart';
import '../../../ui/toast.dart';
import 'glass_chip.dart';
import 'presentation.dart' show failureLabel;
import 'status_feed.dart';
import 'tool_display.dart' show capturedLabel;

/// Package declarations and states come unchanged from the shared Client feed.
class StatusChips extends StatelessWidget {
  const StatusChips({super.key, required this.feed});

  final StatusFeed feed;

  Future<void> _invoke(BuildContext context, Map<String, dynamic> entry) async {
    try {
      await feed.invoke(entry);
    } catch (failure) {
      if (context.mounted) {
        showToast(context, failureLabel(failure, translate: context.tr));
      }
    }
  }

  void _open(BuildContext context, Map<String, dynamic> entry) {
    final state = object(entry['state']);
    final label = state['label'] ?? object(entry['declaration'])['label'];
    if (objects(state['details']).isEmpty && feed.invokable(entry)) {
      _invoke(context, entry);
      return;
    }
    showAppSheet<void>(
      context,
      capturedLabel(context, label),
      child: ListenableBuilder(
        listenable: Listenable.merge([feed, feed.host]),
        builder: (context, _) {
          final selected = feed.current(entry) ?? entry;
          final details = objects(object(selected['state'])['details']);
          return Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (details.isNotEmpty) StatusDetails(details: details),
              if (feed.invokable(selected)) const SizedBox(height: 16),
              if (feed.invokable(selected))
                FilledButton(
                  onPressed: feed.enabled(selected)
                      ? () => _invoke(context, selected)
                      : null,
                  child: Text(
                    feed.retry(selected)
                        ? context.tr('retry')
                        : capturedLabel(context, label),
                  ),
                ),
            ],
          );
        },
      ),
    );
  }

  Color _tone(BuildContext context, String tone) => switch (tone) {
    'accent' => Theme.of(context).colorScheme.primary,
    'success' => SailryTheme.statusColor(context, StatusTone.success),
    'warning' => SailryTheme.statusColor(context, StatusTone.warning),
    'danger' => SailryTheme.statusColor(context, StatusTone.danger),
    _ => Theme.of(context).colorScheme.onSurfaceVariant,
  };

  String? _icon(Object? value) {
    if (value is! String) return null;
    return catalogIcon(value) ??
        (catalogIcons.values.contains(value) ? value : null);
  }

  Widget _chip(BuildContext context, Map<String, dynamic> entry) {
    final declaration = object(entry['declaration']);
    final state = object(entry['state']);
    final label = capturedLabel(
      context,
      state['label'] ?? declaration['label'],
    );
    final icon = _icon(state['icon'] ?? declaration['icon']);
    final segments = objects(state['segments']);
    final enabled = feed.enabled(entry);
    final retry = feed.retry(entry);
    final colors = Theme.of(context).colorScheme;
    final interactive =
        objects(state['details']).isNotEmpty || feed.invokable(entry);
    return Semantics(
      label: retry ? '$label ${context.tr('retry')}' : label,
      child: GlassChip(
        onTap: enabled && interactive ? () => _open(context, entry) : null,
        child: Row(
          mainAxisSize: MainAxisSize.min,
          spacing: 6,
          children: [
            if (icon != null) AppIcon(icon, size: 15),
            if (segments.isEmpty)
              Text(label)
            else
              for (final segment in segments)
                Text(
                  capturedLabel(context, segment['label']),
                  style: TextStyle(
                    color: enabled
                        ? _tone(context, text(segment['tone']))
                        : colors.onSurface.withValues(alpha: .38),
                  ),
                ),
            if (retry) Text(context.tr('retry')),
          ],
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) => Row(
    mainAxisSize: MainAxisSize.min,
    spacing: 8,
    children: [for (final entry in feed.entries) _chip(context, entry)],
  );
}

/// Declared details: short values as number tiles, longer ones as rows.
class StatusDetails extends StatelessWidget {
  const StatusDetails({super.key, required this.details});

  final List<Map<String, dynamic>> details;

  static bool _short(Map<String, dynamic> detail) =>
      detail['progress'] == null && text(detail['value']).length <= 10;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final fill = colors.onSurface.withValues(alpha: .045);
    final tiles = details.where(_short).toList();
    final rows = details.where((detail) => !_short(detail)).toList();
    Widget tile(Map<String, dynamic> detail) => Expanded(
      child: Container(
        padding: const EdgeInsets.fromLTRB(14, 12, 14, 12),
        decoration: BoxDecoration(
          color: fill,
          borderRadius: BorderRadius.circular(14),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          spacing: 4,
          children: [
            Text(
              text(detail['value'], '—'),
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: TextStyle(
                fontSize: 22,
                fontWeight: FontWeight.w600,
                color: colors.onSurface,
                fontFeatures: const [FontFeature.tabularFigures()],
              ),
            ),
            Text(
              capturedLabel(context, detail['label']),
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: TextStyle(fontSize: 12, color: colors.onSurfaceVariant),
            ),
          ],
        ),
      ),
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      spacing: 8,
      children: [
        for (var start = 0; start < tiles.length; start += 3)
          Row(
            spacing: 8,
            children: [
              for (final detail in tiles.skip(start).take(3)) tile(detail),
            ],
          ),
        if (rows.isNotEmpty)
          Container(
            padding: const EdgeInsets.symmetric(horizontal: 14),
            decoration: BoxDecoration(
              color: fill,
              borderRadius: BorderRadius.circular(14),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                for (final (index, detail) in rows.indexed)
                  DecoratedBox(
                    decoration: BoxDecoration(
                      border: index == 0
                          ? null
                          : Border(
                              top: BorderSide(color: colors.outlineVariant),
                            ),
                    ),
                    child: Padding(
                      padding: const EdgeInsets.symmetric(vertical: 12),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.stretch,
                        spacing: 6,
                        children: [
                          Text(
                            capturedLabel(context, detail['label']),
                            style: TextStyle(
                              fontSize: 12,
                              color: colors.onSurfaceVariant,
                            ),
                          ),
                          if (text(detail['value']).isNotEmpty)
                            SelectableText(
                              text(detail['value']),
                              style: TextStyle(color: colors.onSurface),
                            ),
                          if (detail['progress'] case final num progress)
                            LinearProgressIndicator(value: progress / 100),
                        ],
                      ),
                    ),
                  ),
              ],
            ),
          ),
      ],
    );
  }
}
