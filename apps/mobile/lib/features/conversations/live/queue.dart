import 'package:flutter/material.dart';
import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/kit.dart';
import '../../../ui/toast.dart';
import 'presentation.dart' show ConversationCommand, failureLabel;

class LiveQueue extends StatefulWidget {
  const LiveQueue({
    super.key,
    required this.view,
    required this.session,
    required this.command,
  });
  final ValueNotifier<Map<String, dynamic>> view;
  final String session;
  final ConversationCommand command;
  @override
  State<LiveQueue> createState() => _LiveQueueState();
}

class _LiveQueueState extends State<LiveQueue> {
  bool _busy = false;
  String? _error;

  Future<void> _run(String kind, Map<String, dynamic> data) async {
    if (_busy) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await widget.command(kind, data);
    } catch (error) {
      if (mounted) setState(() => _error = failureLabel(error));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _edit(Map<String, dynamic> item) async {
    try {
      final message = object(
        (await widget.command('read_queued_turn', {
          'turn': item['turn'],
        }))['data'],
      );
      if (!mounted) return;
      final input = Map<String, dynamic>.of(object(message['message']));
      var draft = input['text'] as String? ?? '';
      var pending = false;
      await showAppSheet(
        context,
        tr('edit'),
        child: StatefulBuilder(
          builder: (context, update) {
            return Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                TextFormField(
                  initialValue: draft,
                  onChanged: (value) => draft = value,
                  autofocus: true,
                  minLines: 3,
                  maxLines: 6,
                ),
                const SizedBox(height: 16),
                FilledButton(
                  onPressed: pending
                      ? null
                      : () async {
                          update(() => pending = true);
                          try {
                            await widget.command('edit_queued_turn', {
                              'turn': item['turn'],
                              'expected_revision': message['revision'],
                              'message': {...input, 'text': draft},
                            });
                            if (context.mounted) Navigator.pop(context);
                          } catch (error) {
                            if (context.mounted) {
                              update(() => pending = false);
                              showToast(context, failureLabel(error));
                            }
                          }
                        },
                  child: Text(tr('save')),
                ),
              ],
            );
          },
        ),
      );
    } catch (error) {
      if (mounted) setState(() => _error = failureLabel(error));
    }
  }

  @override
  Widget build(BuildContext context) => ValueListenableBuilder(
    valueListenable: widget.view,
    builder: (context, view, _) {
      final queue = object(object(object(view['snapshot'])['page'])['queue']);
      final items = objects(queue['items']);
      final enabled = !_busy && view['connected'] == true;
      return Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (_error != null)
            Text(
              _error!,
              style: TextStyle(color: Theme.of(context).colorScheme.error),
            ),
          if (items.isEmpty)
            EmptyState(icon: 'chat', message: tr('queueEmpty')),
          for (final (index, item) in items.indexed)
            Padding(
              padding: const EdgeInsets.only(bottom: 10),
              child: Surface(
                radius: 16,
                padding: const EdgeInsets.all(12),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    InkWell(
                      onTap: enabled ? () => _edit(item) : null,
                      child: Text(
                        item['preview'] as String? ?? '',
                        style: const TextStyle(height: 1.5),
                      ),
                    ),
                    Wrap(
                      children: [
                        TextButton(
                          onPressed: enabled ? () => _edit(item) : null,
                          child: Text(tr('edit')),
                        ),
                        TextButton(
                          onPressed: enabled
                              ? () => _run('remove_queued_turn', {
                                  'turn': item['turn'],
                                  'expected_revision': item['revision'],
                                })
                              : null,
                          child: Text(tr('delete')),
                        ),
                        if (index > 0)
                          TextButton(
                            onPressed: enabled
                                ? () => _run('move_queued_turn', {
                                    'session': widget.session,
                                    'expected_revision': queue['revision'],
                                    'turn': item['turn'],
                                    'before': items[index - 1]['turn'],
                                  })
                                : null,
                            child: Text(tr('moveUp')),
                          ),
                        TextButton(
                          onPressed: enabled
                              ? () => _run('send_queued_turn', {
                                  'turn': item['turn'],
                                  'expected_revision': item['revision'],
                                })
                              : null,
                          child: Text(tr('send')),
                        ),
                      ],
                    ),
                  ],
                ),
              ),
            ),
          if (items.isNotEmpty)
            OutlinedButton(
              onPressed: enabled
                  ? () => _run('set_queue_paused', {
                      'session': widget.session,
                      'expected_revision': queue['revision'],
                      'paused': queue['paused'] != true,
                    })
                  : null,
              child: Text(
                tr(queue['paused'] == true ? 'resumeQueue' : 'pauseQueue'),
              ),
            ),
        ],
      );
    },
  );
}
