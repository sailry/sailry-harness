import 'package:flutter/foundation.dart' show ValueListenable;
import 'package:flutter/material.dart';

import '../../../content/code.dart';
import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../ui/kit.dart';
import '../../../ui/toast.dart';
import '../../resources/file_page.dart';
import 'message_data.dart';
import 'presentation.dart';
import 'question.dart';
import 'question_history.dart';
import 'tool_content.dart';
import 'tool_display.dart';
import 'tool_heading.dart';
import 'tool_output.dart';
import 'tool_row.dart';

/// The loaded detail, arguments and result of one call projection.
typedef ToolView = ({
  Map<String, dynamic> call,
  Map<String, dynamic>? detail,
  Map<String, dynamic> arguments,
  Map<String, dynamic> result,
});

ToolView toolView(Map<String, dynamic> value, Map<String, dynamic> page) {
  final detail = toolDetail(value);
  final call = {
    ...value,
    if (detail != null) ...{
      'resolved': detail['resolved'],
      'content': detail['content'],
      'texts': detail['texts'],
    },
  };
  return (
    call: call,
    detail: detail,
    arguments: object(
      detail?['arguments'] ??
          object(toolPart(page, call['source'])['summary'])['question'],
    ),
    result: {
      ...toolPart(page, call['response']),
      if (detail != null) 'result': detail['result'],
    },
  );
}

/// Outputs open in a sheet; only approvals and questions stay inline.
bool toolOpens(Map<String, dynamic> call) {
  final question = object(call['question']);
  return (call['details'] == true && hasToolContent(call)) ||
      (question.isNotEmpty && object(question['state'])['kind'] != 'pending');
}

class ToolRecord extends StatefulWidget {
  const ToolRecord({
    super.key,
    this.documents,
    required this.call,
    required this.page,
    required this.session,
    required this.host,
    required this.command,
    required this.enabled,
    this.continuing = false,
    this.interactive = true,
    this.toolOutput,
    required this.updates,
  });
  final Map<String, FileDocument>? documents;
  final Map<String, dynamic> call;
  final Map<String, dynamic> page;
  final Map<String, dynamic> session;
  final HostConnection host;
  final ConversationCommand command;
  final bool enabled;
  final bool continuing;

  /// Whether this row carries the inline approval or question card.
  final bool interactive;
  final ToolSelectionChanged? toolOutput;
  final ValueListenable<Map<String, dynamic>> updates;
  @override
  State<ToolRecord> createState() => _ToolRecordState();
}

class _ToolRecordState extends State<ToolRecord> {
  bool _busy = false;
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
    widget.toolOutput?.call(
      _selection,
      widget.enabled &&
              widget.interactive &&
              pending &&
              TickerMode.valuesOf(context).enabled
          ? [widget.call]
          : [],
      retry,
    );
  }

  @override
  void dispose() {
    widget.toolOutput?.call(_selection, [], false);
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

  void _open() {
    showAppSheet(
      context,
      toolTitle(context, widget.call),
      child: ToolSheet(
        source: object(widget.call['source']),
        updates: widget.updates,
        session: widget.session,
        host: widget.host,
        documents: widget.documents,
        toolOutput: widget.toolOutput,
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final view = toolView(widget.call, widget.page);
    final call = view.call;
    final approval = object(call['approval']);
    final question = object(call['question']);
    final pendingApproval =
        widget.interactive && approval['state'] == 'pending';
    final pendingQuestion =
        widget.interactive && object(question['state'])['kind'] == 'pending';
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
          page: widget.page,
          running: widget.continuing || call['state'] == 'running',
          onTap: toolOpens(call) ? _open : null,
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
            else if (view.detail != null)
              CodeBlock(
                rawText(view.arguments),
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
            Text(view.arguments['prompt'] as String? ?? ''),
            Align(
              alignment: Alignment.centerRight,
              child: FilledButton(
                onPressed: enabled
                    ? () => showAppSheet(
                        context,
                        context.tr('question'),
                        child: ConversationQuestion(
                          spec: view.arguments,
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

/// A call's output, following the live projection while the sheet is open.
class ToolSheet extends StatefulWidget {
  const ToolSheet({
    super.key,
    required this.source,
    required this.updates,
    required this.session,
    required this.host,
    this.documents,
    this.toolOutput,
  });

  final Map<String, dynamic> source;
  final ValueListenable<Map<String, dynamic>> updates;
  final Map<String, dynamic> session;
  final HostConnection host;
  final Map<String, FileDocument>? documents;
  final ToolSelectionChanged? toolOutput;

  @override
  State<ToolSheet> createState() => _ToolSheetState();
}

class _ToolSheetState extends State<ToolSheet> {
  final _selection = Object();

  @override
  void initState() {
    super.initState();
    widget.updates.addListener(_select);
    _select();
  }

  @override
  void dispose() {
    widget.updates.removeListener(_select);
    widget.toolOutput?.call(_selection, [], false);
    super.dispose();
  }

  Map<String, dynamic>? get _call =>
      objects(widget.updates.value['calls']).where((call) {
        final source = object(call['source']);
        return source['entry'] == widget.source['entry'] &&
            source['index'] == widget.source['index'];
      }).firstOrNull;

  bool get _connected =>
      widget.host.connected && widget.updates.value['connected'] == true;

  void _select({bool retry = false}) {
    final call = _call;
    widget.toolOutput?.call(
      _selection,
      _connected && call != null && call['details'] == true ? [call] : [],
      retry,
    );
  }

  @override
  Widget build(BuildContext context) => ValueListenableBuilder(
    valueListenable: widget.updates,
    builder: (context, value, _) {
      final current = _call;
      if (current == null) return const SizedBox.shrink();
      final page = object(object(value['snapshot'])['page']);
      final view = toolView(current, page);
      final call = view.call;
      final question = object(call['question']);
      final resolved = object(call['resolved']);
      final images = objects(view.result['images']);
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          ToolHeading(call: call, page: page),
          const SizedBox(height: 8),
          if (question.isNotEmpty)
            QuestionHistory(question: question, spec: view.arguments)
          else if (resolved['label'] != null)
            ToolInput(input: object(resolved['input']))
          else if (view.arguments.isNotEmpty && images.isEmpty)
            CodeBlock(
              rawText(view.arguments),
              language: 'json',
              title: context.tr('toolArguments'),
              style: TextStyle(
                color: Theme.of(context).colorScheme.onSurfaceVariant,
              ),
            ),
          if (view.detail != null && call['details'] == true)
            ToolContent(
              documents: widget.documents,
              call: call,
              result: view.result,
              output: object(view.detail!['output']),
              host: widget.host,
              worktree: turnWorktree(page, widget.session, call['turn']),
            ),
          if (detailFailed(current))
            Align(
              alignment: Alignment.centerLeft,
              child: TextButton(
                onPressed: _connected ? () => _select(retry: true) : null,
                child: Text(context.tr('retry')),
              ),
            ),
        ],
      );
    },
  );
}
