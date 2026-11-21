import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/kit.dart';
import '../../../ui/toast.dart';
import 'change_preview.dart';
import 'presentation.dart';
import 'question.dart';
import 'tool_display.dart';
import 'tool_heading.dart';
import 'tool_output.dart';
import 'tool_row.dart';

/// A call's loaded arguments, or an inline question's bounded specification.
/// An approval card never shows them; a question card reads its prompt here.
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
    this.background,
    required this.page,
    required this.session,
    required this.command,
    this.onPlanFeedback,
    this.pendingPlanFeedback,
    required this.enabled,
    this.continuing = false,
    this.interactive = true,
    this.toolOutput,
  });
  final Map<String, dynamic> call;
  final Map<String, dynamic>? background;
  final Map<String, dynamic> page;
  final Map<String, dynamic> session;
  final ConversationCommand command;
  final Future<void> Function(String question, String message)? onPlanFeedback;
  final String? Function(String question)? pendingPlanFeedback;
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

  bool get _failed {
    if (widget.call['state'] != 'returned') return false;
    final row = object(widget.call['row']);
    return row['failed'] == true ||
        row['warning'] == true && row['state'] == null;
  }

  bool get _permissionDenied =>
      object(widget.call['failure'])['code'] == 'permission_denied';

  // Approvals use loaded arguments. A failure reads as its summary message or
  // a short label, so the transcript never loads a raw result.
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
    final arguments = pendingQuestion
        ? toolArguments(call, widget.page)
        : const <String, dynamic>{};
    final approvalView = object(call['approval_view']);
    final enabled = widget.enabled && !_busy;
    final colors = Theme.of(context).colorScheme;
    // A command's row state is its outcome; other tools keep the message.
    final failed = _failed && !commandTool(call, widget.background);
    final failure = failed ? text(call['error']).trim() : '';
    final failureLabel = failure.isNotEmpty
        ? failure.split('\n').first
        : toolStateLabel(object(call['row'])['state']) == null
        ? context.tr(_permissionDenied ? 'failureDenied' : 'toolFailed')
        : null;
    // One glass card; code reads as plain monospace text inside it rather
    // than a second framed block.
    Widget card(List<Widget> children) => Padding(
      padding: const EdgeInsets.only(top: 6, bottom: 10),
      child: Surface(
        radius: 18,
        padding: const EdgeInsets.fromLTRB(16, 14, 16, 12),
        child: DefaultTextStyle.merge(
          style: TextStyle(
            fontSize: transcriptFontSize,
            height: 1.5,
            color: colors.onSurface,
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            spacing: 8,
            children: children,
          ),
        ),
      ),
    );
    Widget code(String value) => ConstrainedBox(
      constraints: const BoxConstraints(maxHeight: 220),
      child: SingleChildScrollView(
        child: SelectableText(
          value,
          style: const TextStyle(
            fontFamily: 'monospace',
            fontSize: 13,
            height: 1.5,
          ),
        ),
      ),
    );
    Widget actions(List<Widget> buttons) => Padding(
      padding: const EdgeInsets.only(top: 4),
      child: Row(
        spacing: 8,
        children: [for (final button in buttons) Expanded(child: button)],
      ),
    );
    final muted = TextStyle(color: colors.onSurfaceVariant);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        ToolHeading(
          call: call,
          background: widget.background,
          running:
              widget.continuing ||
              call['state'] == 'running' ||
              widget.enabled && widget.background?['state'] == 'running',
        ),
        if (failed && failureLabel != null)
          Padding(
            padding: const EdgeInsets.only(left: 24, bottom: 4),
            child: Text(
              failureLabel,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: TextStyle(
                fontSize: transcriptFontSize,
                color: rowColor(
                  context,
                  toolFailed(call) ? RowTone.failed : RowTone.pending,
                ),
              ),
            ),
          ),
        if (pendingApproval)
          card([
            // The validated file list replaces the package's generic prompt.
            // A tool that declares no display shows its name alone, never
            // its arguments.
            if (approvalView['changes'] != null)
              ChangePreview(changes: object(approvalView['changes']))
            else if (approvalView['description'] != null)
              approvalView['code'] == true
                  ? code(capturedLabel(context, approvalView['description']))
                  : SelectableText(
                      capturedLabel(context, approvalView['description']),
                    ),
            if (approvalView['context'] != null)
              SelectableText(text(approvalView['context']), style: muted),
            for (final prompt in objects(approvalView['notices']))
              SelectableText(capturedLabel(context, prompt), style: muted),
            if (detailFailed(widget.call))
              Align(
                alignment: Alignment.centerLeft,
                child: TextButton(
                  onPressed: enabled ? () => _select(retry: true) : null,
                  child: Text(context.tr('retry')),
                ),
              ),
            actions([
              FilledButton.tonal(
                onPressed: enabled && detailAllowsDecision(widget.call, 'deny')
                    ? () => _approve('deny')
                    : null,
                child: Text(context.tr('deny')),
              ),
              FilledButton(
                onPressed:
                    enabled && detailAllowsDecision(widget.call, 'approve')
                    ? () => _approve('approve')
                    : null,
                child: Text(context.tr('allowShort')),
              ),
            ]),
          ]),
        if (pendingQuestion)
          card([
            Text(arguments['prompt'] as String? ?? ''),
            actions([
              FilledButton(
                onPressed: enabled
                    ? () => showAppSheet(
                        context,
                        context.tr('question'),
                        child: ConversationQuestion(
                          spec: arguments,
                          question: question,
                          session: widget.session,
                          command: widget.command,
                          onPlanFeedback:
                              call['name'] == 'plan_review' &&
                                  objects(widget.page['runs']).any(
                                    (run) =>
                                        run['turn'] == question['turn'] &&
                                        run['kind'] == 'task' &&
                                        run['status'] == 'completed' &&
                                        run['error'] == null,
                                  )
                              ? widget.onPlanFeedback
                              : null,
                          pendingPlanFeedback: widget.pendingPlanFeedback,
                        ),
                      )
                    : null,
                child: Text(context.tr('reply')),
              ),
            ]),
          ]),
      ],
    );
  }
}
