import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import 'activity.dart';
import 'tool_content.dart';
import 'command_result.dart';
import 'tool_display.dart';

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

bool toolFailed(Map<String, dynamic> call, Map<String, dynamic> page) {
  final result = object(toolPart(page, call['response'])['result']);
  return resultFailed(result, object(call['resolved']));
}

class ToolHeading extends StatelessWidget {
  const ToolHeading({
    super.key,
    required this.call,
    required this.page,
    this.pending,
    this.running,
    this.failed,
  });
  final Map<String, dynamic> call;
  final Map<String, dynamic> page;
  final bool? pending;
  final bool? running;
  final bool? failed;

  @override
  Widget build(BuildContext context) {
    final waiting = pending ?? pendingTool(call);
    final active = (running ?? call['state'] == 'running') && !waiting;
    final result = object(toolPart(page, call['response'])['result']);
    final ownFailure = toolFailed(call, page);
    final hasFailure =
        !waiting && call['state'] != 'running' && (failed ?? ownFailure);
    final resolved = object(call['resolved']);
    final capturedStatus = capturedLabel(context, resolved['status']);
    final resultStatus = switch (object(result['error'])['code']) {
      'cancelled' => tr('toolCancelled'),
      'outcome_unknown' => tr('toolOutcomeUnknown'),
      _ =>
        result['error'] != null || ownFailure
            ? null
            : capturedStatus.isNotEmpty
            ? capturedStatus
            : commandStatus(result),
    };
    final status = waiting
        ? tr('conversationToolWaiting')
        : hasFailure
        ? (ownFailure ? resultStatus : null) ?? tr('toolFailed')
        : resultStatus ??
              tr(switch (call['state']) {
                'returned' => 'conversationToolReturned',
                'cancelled' => 'conversationToolCancelled',
                'not_executed' => 'conversationToolNotExecuted',
                'interrupted' => 'conversationToolInterrupted',
                _ => 'conversationToolWaiting',
              });
    final args = object(toolPart(page, call['source'])['arguments']);
    final summary = text(object(object(resolved['input'])['summary'])['text']);
    final target = resolved['label'] != null
        ? (summary.isEmpty ? null : summary)
        : [
            args['path'],
            args['command'],
            args['query'],
          ].whereType<String>().where((value) => value.isNotEmpty).firstOrNull;
    final label = [
      resolved['label'] != null
          ? capturedLabel(context, resolved['label'])
          : toolLabel(text(call['name'])),
      if (target != null) target.split('\n').first,
    ].join(' ');
    return Semantics(
      value: hasFailure ? tr('toolFailed') : null,
      child: Row(
        children: [
          Expanded(
            child: ActivityLabel(
              label: label,
              running: active,
              color: hasFailure ? Theme.of(context).colorScheme.error : null,
            ),
          ),
          if (!active && !hasFailure) ...[
            const SizedBox(width: 8),
            Text(
              status,
              style: TextStyle(
                color: Theme.of(context).colorScheme.onSurfaceVariant,
                fontSize: 12,
              ),
            ),
          ],
        ],
      ),
    );
  }
}
