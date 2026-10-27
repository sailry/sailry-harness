import 'dart:async';

import '../../resources/file_page.dart';
import 'package:flutter/material.dart';
import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../ui/kit.dart';
import 'presentation.dart';
import 'tool_heading.dart';
import 'disclosure.dart';
import 'question.dart';
import 'question_history.dart';
import '../../../content/code.dart';
import 'tool_content.dart';
import 'message_data.dart';
import 'tool_display.dart';
import 'tool_output.dart';
import '../../../ui/toast.dart';

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
    this.toolOutput,
  });
  final Map<String, FileDocument>? documents;
  final Map<String, dynamic> call;
  final Map<String, dynamic> page;
  final Map<String, dynamic> session;
  final HostConnection host;
  final ConversationCommand command;
  final bool enabled;
  final bool continuing;
  final OutputLoader? toolOutput;
  @override
  State<ToolRecord> createState() => _ToolRecordState();
}

class _ToolRecordState extends State<ToolRecord> {
  bool _busy = false;
  bool _expanded = false;
  final _output = DeferredOutput();

  @override
  void initState() {
    super.initState();
    _output.bind(widget.call);
    _output.addListener(_outputChanged);
    unawaited(_resolveOutput());
  }

  void _outputChanged() {
    if (mounted) setState(() {});
  }

  @override
  void didUpdateWidget(covariant ToolRecord oldWidget) {
    super.didUpdateWidget(oldWidget);
    _output.bind(widget.call);
    unawaited(_resolveOutput());
  }

  Future<void> _resolveOutput({bool retry = false}) async {
    try {
      await _output.resolve(
        widget.call,
        expanded: _expanded,
        enabled: widget.enabled,
        load: widget.toolOutput,
        retry: retry,
      );
    } catch (error) {
      if (mounted &&
          widget.enabled &&
          (_expanded ||
              object(widget.call['approval'])['state'] == 'pending')) {
        showToast(context, failureLabel(error, translate: context.tr));
      }
    }
  }

  @override
  void dispose() {
    _output.dispose();
    super.dispose();
  }

  Future<void> _approve(String decision) async {
    if (_busy || (decision == 'approve' && _output.value == null)) return;
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
    final detail = _output.value;
    final call = {
      ...widget.call,
      if (detail != null) ...{
        'resolved': detail['resolved'],
        'content': detail['content'],
        'texts': detail['texts'],
      },
    };
    final source = object(call['source']);
    final arguments = object(
      detail?['arguments'] ??
          object(toolPart(widget.page, call['source'])['summary'])['question'],
    );
    final result = {
      ...toolPart(widget.page, call['response']),
      if (detail != null) 'result': detail['result'],
    };
    final images = objects(result['images']);
    final resolved = object(call['resolved']);
    final showResult = detail != null;
    final approval = object(call['approval']);
    final question = object(call['question']);
    final pendingApproval = approval['state'] == 'pending';
    final pendingQuestion = object(question['state'])['kind'] == 'pending';
    final details = call['details'] == true;
    final expandable =
        (details && hasToolContent(call)) ||
        pendingApproval ||
        question.isNotEmpty;
    final input = object(resolved['input']);
    final approvalView = object(
      detail?['approval_view'] ?? call['approval_view'],
    );
    final enabled = widget.enabled && !_busy;
    return Padding(
      padding: const EdgeInsets.only(bottom: 8),
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 4),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            WorkDisclosure(
              key: PageStorageKey('tool-${source['entry']}-${source['index']}'),
              autoExpanded: pendingApproval,
              expandable: expandable,
              onChanged: (expanded) {
                _expanded = expanded;
                if (expanded) unawaited(_resolveOutput());
              },
              title: ToolHeading(
                call: call,
                page: widget.page,
                running: widget.continuing || call['state'] == 'running',
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  if (question.isNotEmpty && !pendingQuestion)
                    QuestionHistory(question: question, spec: arguments)
                  else if (pendingQuestion)
                    Align(
                      alignment: Alignment.centerLeft,
                      child: Text(arguments['prompt'] as String? ?? ''),
                    )
                  else if (pendingApproval &&
                      approvalView['description'] != null)
                    approvalView['code'] == true
                        ? CodeBlock(
                            capturedLabel(context, approvalView['description']),
                          )
                        : SelectableText(
                            capturedLabel(context, approvalView['description']),
                          )
                  else if (pendingApproval && detail != null)
                    CodeBlock(
                      rawText(arguments),
                      language: 'json',
                      title: context.tr('toolArguments'),
                    )
                  else if (resolved['label'] != null)
                    ToolInput(input: input)
                  else if (arguments.isNotEmpty && images.isEmpty)
                    CodeBlock(
                      rawText(arguments),
                      language: 'json',
                      title: context.tr('toolArguments'),
                      style: TextStyle(
                        color: Theme.of(context).colorScheme.onSurfaceVariant,
                      ),
                    ),
                  if (showResult && details)
                    ToolContent(
                      documents: widget.documents,
                      call: call,
                      result: result,
                      output: object(detail['output']),
                      host: widget.host,
                      worktree: turnWorktree(
                        widget.page,
                        widget.session,
                        call['turn'],
                      ),
                    ),
                  if (!pendingApproval && _output.failed)
                    Align(
                      alignment: Alignment.centerLeft,
                      child: TextButton(
                        onPressed: enabled
                            ? () => unawaited(_resolveOutput(retry: true))
                            : null,
                        child: Text(context.tr('retry')),
                      ),
                    ),
                ],
              ),
            ),
            if (pendingApproval && approvalView['context'] != null)
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 4),
                child: SelectableText(text(approvalView['context'])),
              ),
            for (final prompt in objects(approvalView['notices']))
              if (pendingApproval)
                Padding(
                  padding: const EdgeInsets.symmetric(vertical: 4),
                  child: SelectableText(capturedLabel(context, prompt)),
                ),
            if (pendingApproval && approvalView['unrestricted'] == true)
              Text(context.tr('conversationFull')),
            if (pendingApproval)
              Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  if (_output.failed)
                    TextButton(
                      onPressed: enabled
                          ? () => unawaited(_resolveOutput(retry: true))
                          : null,
                      child: Text(context.tr('retry')),
                    ),
                  TextButton(
                    onPressed: enabled ? () => _approve('deny') : null,
                    child: Text(context.tr('deny')),
                  ),
                  const SizedBox(width: 8),
                  FilledButton(
                    onPressed: enabled && detail != null
                        ? () => _approve('approve')
                        : null,
                    child: Text(context.tr('allowShort')),
                  ),
                ],
              ),
            if (pendingQuestion)
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
          ],
        ),
      ),
    );
  }
}
