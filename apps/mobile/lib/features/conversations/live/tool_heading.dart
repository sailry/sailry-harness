import 'package:flutter/material.dart';

import '../../../content/code.dart';
import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/kit.dart';
import 'background_data.dart';
import 'command_detail.dart';
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

/// A call's row, including any background command it started.
class ToolHeading extends StatelessWidget {
  const ToolHeading({
    super.key,
    required this.call,
    this.running,
    this.background,
    this.openCommandDetail,
  });
  final Map<String, dynamic> call;
  final bool? running;
  final Map<String, dynamic>? background;
  final OpenCommandDetail? openCommandDetail;

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
    final title = _title(context, call);
    final outcomeLabel = outcome != null
        ? context.tr(backgroundLabel(outcome))
        : task?['state'] == 'running'
        ? context.tr('running')
        : null;
    final exit = task?['exit_code'];
    final status = outcomeLabel != null && exit is int && exit != 0
        ? '$outcomeLabel ($exit)'
        : outcomeLabel;
    final hold = task != null && backgroundHeld(task)
        ? context.tr(backgroundHoldLabel(task['hold']))
        : null;
    final detail = text(object(row['detail'])['text']);
    final summary = text(task?['summary']);
    final hasSummary = summary.trim().isNotEmpty;
    final open = openCommandDetail;
    final inspectable = hasSummary || hold != null || open != null;
    return Semantics(
      value: failed && status == null ? context.tr('toolFailed') : null,
      child: ToolRow(
        leading: (color) => toolIcon(call, color),
        label: title,
        detail: [?status, if (detail.isNotEmpty) detail].join(' · '),
        tone: tone,
        running: active,
        action: inspectable
            ? AppIcon('chevron', size: 12, color: rowColor(context, tone))
            : null,
        onTap: inspectable
            ? () {
                final retained = Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  spacing: 8,
                  children: [
                    if (status != null || open != null)
                      Text(status ?? context.tr('backgroundUnknown')),
                    if (hold != null) Text(hold),
                    if (hasSummary) CodeBlock(summary),
                  ],
                );
                showAppSheet(
                  context,
                  title,
                  child: open == null
                      ? retained
                      : CommandDetailBody(
                          open: open,
                          fallback: retained,
                          hold: hold,
                        ),
                );
              }
            : null,
      ),
    );
  }
}
