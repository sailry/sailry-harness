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
  }

  void _outputChanged() {
    if (mounted) setState(() {});
  }

  @override
  void didUpdateWidget(covariant ToolRecord oldWidget) {
    super.didUpdateWidget(oldWidget);
    _output.bind(widget.call);
    if (_expanded) unawaited(_resolveOutput());
  }

  Future<void> _resolveOutput() async {
    try {
      await _output.resolve(
        widget.call,
        expanded: _expanded,
        enabled: widget.enabled,
        load: widget.toolOutput,
      );
    } catch (error) {
      if (mounted && _expanded && widget.enabled) {
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
    if (_busy) return;
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
    final source = object(call['source']);
    final arguments = object(
      toolPart(widget.page, call['source'])['arguments'],
    );
    final result = toolPart(widget.page, call['response']);
    final images = objects(result['images']);
    final resolved = object(call['resolved']);
    final showResult = result.containsKey('result');
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
    final inputText = {
      for (final field in input.values) text(object(field)['text']),
    };
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
              autoExpanded: false,
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
                      output: _output.value,
                      host: widget.host,
                      worktree: turnWorktree(
                        widget.page,
                        widget.session,
                        call['turn'],
                      ),
                    ),
                ],
              ),
            ),
            for (final prompt in objects(resolved['approval']))
              if (pendingApproval &&
                  !inputText.contains(capturedLabel(context, prompt)))
                Padding(
                  padding: const EdgeInsets.symmetric(vertical: 4),
                  child: SelectableText(capturedLabel(context, prompt)),
                ),
            if (pendingApproval)
              Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  TextButton(
                    onPressed: enabled ? () => _approve('deny') : null,
                    child: Text(context.tr('deny')),
                  ),
                  const SizedBox(width: 8),
                  FilledButton(
                    onPressed: enabled ? () => _approve('approve') : null,
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
