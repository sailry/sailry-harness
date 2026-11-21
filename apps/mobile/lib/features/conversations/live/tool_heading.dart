import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import 'background_data.dart';
import 'tool_display.dart';
import 'tool_row.dart';

bool pendingTool(Map<String, dynamic> call) =>
    object(call['approval'])['state'] == 'pending' ||
    object(object(call['question'])['state'])['kind'] == 'pending';

/// A shell command, whether it ran in the foreground or started a background
/// task. Its row shows the command line alone.
bool commandTool(Map<String, dynamic> call, Map<String, dynamic>? background) =>
    background != null ||
    call['name'] == 'run_command' ||
    call['command_detail'] != null ||
    object(object(call['resolved'])['command']).isNotEmpty;

/// The short label for a Client row state; running and returned rows have none.
String? toolStateLabel(Object? state) => switch (state) {
  'failed' => 'toolFailed',
  'denied' => 'failureDenied',
  'interrupted' => 'backgroundInterrupted',
  'not_executed' => 'conversationStepSkipped',
  'pending' => 'conversationToolWaiting',
  _ => null,
};

String _title(BuildContext context, Map<String, dynamic> call) {
  final label = object(call['row'])['label'];
  final name = text(call['name']);
  return label != null
      ? capturedLabel(context, label)
      : hasLocalization('tool_$name')
      ? context.tr('tool_$name')
      : name;
}

/// A call's single row, including any background command it started. Rows
/// carry a short state, never arguments, output or an expansion.
class ToolHeading extends StatelessWidget {
  const ToolHeading({
    super.key,
    required this.call,
    this.running,
    this.background,
  });
  final Map<String, dynamic> call;
  final bool? running;
  final Map<String, dynamic>? background;

  @override
  Widget build(BuildContext context) {
    final row = object(call['row']);
    final waiting = row['pending'] == true || pendingTool(call);
    final active = (running ?? row['running'] == true) && !waiting;
    final task = background;
    final outcome = task?['outcome'];
    final failed =
        !waiting &&
        (call['state'] != 'running' && toolFailed(call) ||
            outcome != null && backgroundFailed(outcome));
    final warning =
        !waiting &&
        (call['state'] != 'running' && row['warning'] == true ||
            backgroundWarning(outcome));
    final tone = failed
        ? RowTone.failed
        : waiting || warning
        ? RowTone.pending
        : RowTone.normal;
    final outcomeLabel = outcome != null
        ? context.tr(backgroundLabel(outcome))
        : task?['state'] == 'running'
        ? context.tr('running')
        : null;
    final exit = task?['exit_code'];
    final rowState = toolStateLabel(row['state']);
    final command = commandTool(call, task);
    // A command shows no message, so its failure always carries a label.
    final state = outcomeLabel != null && exit is int && exit != 0
        ? '$outcomeLabel ($exit)'
        : outcomeLabel ??
              (task != null && backgroundHeld(task)
                  ? context.tr('backgroundHeld')
                  : rowState != null
                  ? context.tr(rowState)
                  : command && (failed || warning)
                  ? context.tr('toolFailed')
                  : null);
    final detail = text(object(row['detail'])['text']);
    final line = command
        ? detail
              .split('\n')
              .map((line) => line.trim())
              .firstWhere((line) => line.isNotEmpty, orElse: () => '')
        : '';
    return Semantics(
      value: failed && state == null ? context.tr('toolFailed') : null,
      child: ToolRow(
        leading: (color) => toolIcon(call, color),
        label: line.isNotEmpty ? line : _title(context, call),
        detail: command || detail.isEmpty ? null : detail,
        state: state,
        tone: tone,
        running: active,
      ),
    );
  }
}
