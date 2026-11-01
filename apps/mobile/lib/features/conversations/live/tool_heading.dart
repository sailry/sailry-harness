import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import 'tool_content.dart';
import 'command_result.dart';
import 'tool_display.dart';
import 'tool_row.dart';

Map<String, dynamic> toolPart(Map<String, dynamic> page, Object? reference) {
  final source = object(reference);
  final entry = objects(
    page['entries'],
  ).where((entry) => entry['id'] == source['entry']).firstOrNull;
  final parts = objects(entry?['parts']);
  final index = source['index'];
  return index is int && index >= 0 && index < parts.length
      ? object(parts[index]['data'])
      : {};
}

bool pendingTool(Map<String, dynamic> call) =>
    object(call['approval'])['state'] == 'pending' ||
    object(object(call['question'])['state'])['kind'] == 'pending';

String toolTitle(BuildContext context, Map<String, dynamic> call) {
  final label = object(call['row'])['label'];
  return label != null
      ? capturedLabel(context, label)
      : toolLabel(text(call['name']), translate: context.tr);
}

/// A call's collapsed row: success stays silent, settled failures name their state.
class ToolHeading extends StatelessWidget {
  const ToolHeading({
    super.key,
    required this.call,
    required this.page,
    this.running,
    this.onTap,
  });
  final Map<String, dynamic> call;
  final Map<String, dynamic> page;
  final bool? running;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    final row = object(call['row']);
    final waiting = row['pending'] == true || pendingTool(call);
    final active = (running ?? row['running'] == true) && !waiting;
    final failed =
        !waiting &&
        call['state'] != 'running' &&
        (toolFailed(call) || row['warning'] == true);
    final resolved = object(call['resolved']);
    final captured = capturedLabel(context, resolved['status']);
    final status = switch (object(call['failure'])['code']) {
      'cancelled' => context.tr('toolCancelled'),
      'outcome_unknown' => context.tr('toolOutcomeUnknown'),
      _ =>
        captured.isNotEmpty
            ? captured
            : commandStatus(object(resolved['command']), translate: context.tr),
    };
    final state = failed
        ? status ??
              context.tr(switch (row['state']) {
                'cancelled' => 'conversationToolCancelled',
                'interrupted' => 'conversationToolInterrupted',
                'denied' || 'not_executed' => 'conversationToolNotExecuted',
                _ => 'toolFailed',
              })
        : null;
    return Semantics(
      value: failed ? context.tr('toolFailed') : null,
      child: ToolRow(
        leading: (color) => toolIcon(call, color),
        label: toolTitle(context, call),
        detail: text(object(row['detail'])['text']),
        state: state,
        tone: waiting
            ? RowTone.pending
            : failed
            ? RowTone.failed
            : RowTone.normal,
        running: active,
        onTap: onTap,
      ),
    );
  }
}
