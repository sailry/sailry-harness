import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/icons.dart' show catalogIcon, catalogIcons;
import '../../../ui/kit.dart';
import '../../../ui/theme.dart';
import '../../../ui/toast.dart';
import 'presentation.dart' show failureLabel;
import 'status_feed.dart';
import 'tool_display.dart' show capturedLabel;
import 'tool_row.dart' show transcriptFontSize;

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
              for (final detail in details)
                Padding(
                  padding: const EdgeInsets.symmetric(vertical: 8),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    spacing: 6,
                    children: [
                      Text(capturedLabel(context, detail['label'])),
                      if (text(detail['value']).isNotEmpty)
                        SelectableText(text(detail['value'])),
                      if (detail['progress'] case final num progress)
                        LinearProgressIndicator(value: progress / 100),
                    ],
                  ),
                ),
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
      child: OutlinedButton(
        onPressed: enabled && interactive ? () => _open(context, entry) : null,
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
