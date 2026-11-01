import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import 'tool_display.dart';
import 'tool_row.dart';

bool pendingTool(Map<String, dynamic> call) =>
    object(call['approval'])['state'] == 'pending' ||
    object(object(call['question'])['state'])['kind'] == 'pending';

String _title(BuildContext context, Map<String, dynamic> call) {
  final label = object(call['row'])['label'];
  final name = text(call['name']);
  return label != null
      ? capturedLabel(context, label)
      : hasLocalization('tool_$name')
      ? context.tr('tool_$name')
      : name;
}

/// A call's collapsed row: failures show only through their tone.
class ToolHeading extends StatelessWidget {
  const ToolHeading({super.key, required this.call, this.running});
  final Map<String, dynamic> call;
  final bool? running;

  @override
  Widget build(BuildContext context) {
    final row = object(call['row']);
    final waiting = row['pending'] == true || pendingTool(call);
    final active = (running ?? row['running'] == true) && !waiting;
    final failed =
        !waiting &&
        call['state'] != 'running' &&
        (toolFailed(call) || row['warning'] == true);
    return Semantics(
      value: failed ? context.tr('toolFailed') : null,
      child: ToolRow(
        leading: (color) => toolIcon(call, color),
        label: _title(context, call),
        detail: text(object(row['detail'])['text']),
        tone: waiting
            ? RowTone.pending
            : failed
            ? RowTone.failed
            : RowTone.normal,
        running: active,
      ),
    );
  }
}
