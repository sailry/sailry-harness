import 'package:flutter/material.dart';
import 'package:url_launcher/url_launcher.dart';
import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import 'presentation.dart' show ConversationCommand, failureLabel;

class ConversationQuestion extends StatefulWidget {
  const ConversationQuestion({
    super.key,
    required this.spec,
    required this.question,
    required this.session,
    required this.command,
  });
  final Map<String, dynamic> spec;
  final Map<String, dynamic> question;
  final Map<String, dynamic> session;
  final ConversationCommand command;
  @override
  State<ConversationQuestion> createState() => _ConversationQuestionState();
}

class _ConversationQuestionState extends State<ConversationQuestion> {
  final _text = TextEditingController();
  final Set<int> _selected = {};
  final Map<String, dynamic> _form = {};
  bool _busy = false;
  String? _error;
  @override
  void initState() {
    super.initState();
    for (final field in objects(object(widget.spec['input'])['fields'])) {
      if (field['required'] == true &&
          object(field['input'])['kind'] == 'boolean') {
        _form[field['name'] as String] = false;
      }
    }
  }

  @override
  void dispose() {
    _text.dispose();
    super.dispose();
  }

  Future<void> _send(Map<String, dynamic> response) async {
    if (_busy) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await widget.command('resolve_question', {
        'session': widget.session['id'],
        'question': widget.question['id'],
        'response': response,
      });
      if (mounted) Navigator.pop(context);
    } catch (error) {
      if (mounted) {
        setState(() {
          _busy = false;
          _error = failureLabel(error, translate: context.tr);
        });
      }
    }
  }

  Widget _field(Map<String, dynamic> field) {
    final input = object(field['input']);
    final name = field['name'] as String;
    final title = '${field['title']}${field['required'] == true ? ' *' : ''}';
    switch (input['kind']) {
      case 'boolean':
        return CheckboxListTile(
          contentPadding: EdgeInsets.zero,
          title: Text(title),
          value: _form[name] == true,
          onChanged: _busy
              ? null
              : (value) => setState(() => _form[name] = value),
        );
      case 'choice':
        final multiple = input['multiple'] == true;
        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(title),
            for (final choice in objects(input['options']))
              CheckboxListTile(
                contentPadding: EdgeInsets.zero,
                title: Text(choice['title'] as String),
                value: multiple
                    ? (_form[name] as List? ?? []).contains(choice['value'])
                    : _form[name] == choice['value'],
                onChanged: _busy
                    ? null
                    : (checked) => setState(() {
                        if (multiple) {
                          final values = List<String>.from(
                            _form[name] as List? ?? [],
                          );
                          checked == true
                              ? values.add(choice['value'] as String)
                              : values.remove(choice['value']);
                          _form[name] = values;
                        } else {
                          _form[name] = checked == true
                              ? choice['value']
                              : null;
                          if (_form[name] == null) _form.remove(name);
                        }
                      }),
              ),
          ],
        );
      default:
        return Padding(
          padding: const EdgeInsets.only(bottom: 12),
          child: TextFormField(
            enabled: !_busy,
            decoration: InputDecoration(labelText: title),
            keyboardType: input['kind'] == 'number'
                ? const TextInputType.numberWithOptions(
                    decimal: true,
                    signed: true,
                  )
                : TextInputType.text,
            onChanged: (value) {
              if (value.isEmpty && field['required'] != true) {
                _form.remove(name);
                return;
              }
              _form[name] = input['kind'] == 'number'
                  ? (input['integer'] == true
                            ? int.tryParse(value)
                            : num.tryParse(value)) ??
                        value
                  : value;
            },
          ),
        );
    }
  }

  @override
  Widget build(BuildContext context) {
    final input = object(widget.spec['input']);
    final kind = input['kind'];
    final options = (input['options'] as List?)?.cast<String>() ?? [];
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        SelectableText(widget.spec['prompt'] as String? ?? ''),
        const SizedBox(height: 16),
        if (kind == 'choice')
          for (final (index, option) in options.indexed)
            CheckboxListTile(
              contentPadding: EdgeInsets.zero,
              title: Text(option),
              value: _selected.contains(index),
              onChanged: _busy
                  ? null
                  : (checked) => setState(() {
                      if (input['multiple'] != true) {
                        _selected.clear();
                        if (checked == true) _text.clear();
                      }
                      checked == true
                          ? _selected.add(index)
                          : _selected.remove(index);
                    }),
            ),
        if (kind == 'text' ||
            kind == 'plan' ||
            kind == 'choice' && input['allow_other'] == true)
          TextField(
            controller: _text,
            enabled: !_busy,
            onChanged: kind == 'choice' && input['multiple'] != true
                ? (_) => setState(_selected.clear)
                : null,
            minLines: input['multiline'] == true || kind == 'plan' ? 3 : 1,
            maxLines: input['multiline'] == true || kind == 'plan' ? 6 : 1,
            decoration: InputDecoration(
              hintText: kind == 'plan'
                  ? context.tr('conversationPlanFeedback')
                  : kind == 'choice'
                  ? context.tr('conversationOther')
                  : null,
            ),
          ),
        if (kind == 'form')
          for (final field in objects(input['fields'])) _field(field),
        if (kind == 'url')
          OutlinedButton(
            onPressed: _busy
                ? null
                : () async {
                    try {
                      final uri = Uri.parse(input['url'] as String);
                      if (!await launchUrl(
                        uri,
                        mode: LaunchMode.externalApplication,
                      )) {
                        throw StateError(context.tr('conversationFailed'));
                      }
                      // Opening is consent, not a claim that the external workflow succeeded.
                      await _send({
                        'kind': 'answer',
                        'data': {'kind': 'opened'},
                      });
                    } catch (error) {
                      if (mounted) {
                        setState(
                          () => _error = failureLabel(
                            error,
                            translate: context.tr,
                          ),
                        );
                      }
                    }
                  },
            child: Text(input['url'] as String),
          ),
        if (_error != null)
          Padding(
            padding: const EdgeInsets.only(top: 12),
            child: Text(
              _error!,
              style: TextStyle(color: Theme.of(context).colorScheme.error),
            ),
          ),
        const SizedBox(height: 16),
        Wrap(
          alignment: WrapAlignment.end,
          spacing: 8,
          children: [
            TextButton(
              onPressed: _busy ? null : () => _send({'kind': 'cancel'}),
              child: Text(context.tr('cancel')),
            ),
            TextButton(
              onPressed: _busy ? null : () => _send({'kind': 'decline'}),
              child: Text(context.tr('deny')),
            ),
            if (kind == 'plan')
              FilledButton(
                onPressed: _busy
                    ? null
                    : () => _send({
                        'kind': 'start_coding',
                        'data': {
                          'expected_revision': widget.session['revision'],
                          'message': {'text': _text.text, 'attachments': []},
                        },
                      }),
                child: Text(context.tr('conversationStartCoding')),
              ),
            if (kind != 'url')
              FilledButton(
                onPressed: _busy
                    ? null
                    : () {
                        final answer = switch (kind) {
                          'choice' => {
                            'kind': 'choices',
                            'data': {
                              'selected': _selected.toList()..sort(),
                              'other': _text.text.trim().isEmpty
                                  ? null
                                  : _text.text,
                            },
                          },
                          'form' => {'kind': 'form', 'data': _form},
                          _ => {'kind': 'text', 'data': _text.text},
                        };
                        _send({'kind': 'answer', 'data': answer});
                      },
                child: Text(context.tr('conversationSubmit')),
              ),
          ],
        ),
      ],
    );
  }
}
