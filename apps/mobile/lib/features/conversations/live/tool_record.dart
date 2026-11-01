import 'dart:convert';

import 'package:flutter/material.dart';

import '../../../content/code.dart';
import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/kit.dart';
import '../../../ui/toast.dart';
import 'presentation.dart';
import 'question.dart';
import 'tool_display.dart';
import 'tool_heading.dart';
import 'tool_output.dart';
import 'tool_row.dart';

/// Approval arguments or an inline question's bounded specification.
Map<String, dynamic> toolArguments(
  Map<String, dynamic> call,
  Map<String, dynamic> page,
) => object(
  toolDetail(call)?['arguments'] ??
      object(_part(page, call['source'])['summary'])['question'],
);

Map<String, dynamic> _part(Map<String, dynamic> page, Object? reference) {
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

class ToolRecord extends StatefulWidget {
  const ToolRecord({
    super.key,
    required this.call,
    required this.page,
    required this.session,
    required this.command,
    required this.enabled,
    this.continuing = false,
    this.interactive = true,
    this.toolOutput,
  });
  final Map<String, dynamic> call;
  final Map<String, dynamic> page;
  final Map<String, dynamic> session;
  final ConversationCommand command;
  final bool enabled;
  final bool continuing;

  /// Whether this row carries the inline approval or question card.
  final bool interactive;
  final ToolSelectionChanged? toolOutput;
  @override
  State<ToolRecord> createState() => _ToolRecordState();
}

class _ToolRecordState extends State<ToolRecord> {
  bool _busy = false;
  bool _tracking = false;
  final _selection = Object();

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _select();
  }

  @override
  void didUpdateWidget(covariant ToolRecord oldWidget) {
    super.didUpdateWidget(oldWidget);
    _select();
  }

  // Approval decisions need the loaded arguments before they can be allowed.
  void _select({bool retry = false}) {
    final pending = object(widget.call['approval'])['state'] == 'pending';
    final selected =
        widget.enabled &&
        widget.interactive &&
        pending &&
        TickerMode.valuesOf(context).enabled;
    if (!selected && !_tracking) return;
    _tracking = selected;
    widget.toolOutput?.call(_selection, selected ? [widget.call] : [], retry);
  }

  @override
  void dispose() {
    if (_tracking) widget.toolOutput?.call(_selection, [], false);
    super.dispose();
  }

  Future<void> _approve(String decision) async {
    if (_busy || !detailAllowsDecision(widget.call, decision)) {
      return;
    }
    setState(() {
      _busy = true;
    });
    try {
      await widget.command('resolve_approval', {
        'session': widget.session['id'],
        'approval': object(widget.call['approval'])['id'],
        'decision': decision,
      });
    } catch (error) {
      if (mounted) {
        showToast(context, failureLabel(error, translate: context.tr));
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final call = widget.call;
    final approval = object(call['approval']);
    final question = object(call['question']);
    final pendingApproval =
        widget.interactive && approval['state'] == 'pending';
    final pendingQuestion =
        widget.interactive && object(question['state'])['kind'] == 'pending';
    final arguments = pendingApproval || pendingQuestion
        ? toolArguments(call, widget.page)
        : const <String, dynamic>{};
    final approvalView = object(call['approval_view']);
    final enabled = widget.enabled && !_busy;
    final colors = Theme.of(context).colorScheme;
    Widget card(List<Widget> children) => Container(
      margin: const EdgeInsets.only(top: 4, bottom: 8),
      padding: const EdgeInsets.fromLTRB(14, 12, 14, 4),
      decoration: BoxDecoration(
        color: colors.onSurface.withValues(alpha: .05),
        borderRadius: BorderRadius.circular(14),
      ),
      child: DefaultTextStyle.merge(
        style: const TextStyle(fontSize: transcriptFontSize),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: children,
        ),
      ),
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        ToolHeading(
          call: call,
          running: widget.continuing || call['state'] == 'running',
        ),
        if (pendingApproval)
          card([
            if (approvalView['description'] != null)
              approvalView['code'] == true
                  ? CodeBlock(
                      capturedLabel(context, approvalView['description']),
                    )
                  : SelectableText(
                      capturedLabel(context, approvalView['description']),
                    )
            else if (toolDetail(call) != null)
              CodeBlock(
                const JsonEncoder.withIndent('  ').convert(arguments),
                language: 'json',
                title: context.tr('toolArguments'),
              ),
            if (approvalView['context'] != null)
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 4),
                child: SelectableText(text(approvalView['context'])),
              ),
            for (final prompt in objects(approvalView['notices']))
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 4),
                child: SelectableText(capturedLabel(context, prompt)),
              ),
            if (approvalView['unrestricted'] == true)
              Text(context.tr('conversationFull')),
            Row(
              mainAxisAlignment: MainAxisAlignment.end,
              children: [
                if (detailFailed(widget.call))
                  TextButton(
                    onPressed: enabled ? () => _select(retry: true) : null,
                    child: Text(context.tr('retry')),
                  ),
                TextButton(
                  onPressed:
                      enabled && detailAllowsDecision(widget.call, 'deny')
                      ? () => _approve('deny')
                      : null,
                  child: Text(context.tr('deny')),
                ),
                const SizedBox(width: 8),
                FilledButton(
                  onPressed:
                      enabled && detailAllowsDecision(widget.call, 'approve')
                      ? () => _approve('approve')
                      : null,
                  child: Text(context.tr('allowShort')),
                ),
              ],
            ),
          ]),
        if (pendingQuestion)
          card([
            Text(arguments['prompt'] as String? ?? ''),
            Align(
              alignment: Alignment.centerRight,
              child: FilledButton(
                onPressed: enabled
                    ? () => showAppSheet(
                        context,
                        context.tr('question'),
                        child: ConversationQuestion(
                          spec: arguments,
                          question: question,
                          session: widget.session,
                          command: widget.command,
                        ),
                      )
                    : null,
                child: Text(context.tr('reply')),
              ),
            ),
          ]),
      ],
    );
  }
}
