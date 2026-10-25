import 'package:flutter/material.dart';
import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/kit.dart';
import '../../../ui/toast.dart';
import 'presentation.dart' show ConversationCommand, failureLabel;

class QueueSummary extends StatelessWidget {
  const QueueSummary({super.key, required this.queue, required this.onOpen});

  final Map<String, dynamic> queue;
  final VoidCallback onOpen;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.symmetric(horizontal: 14),
    child: Surface(
      radius: 18,
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
      onTap: onOpen,
      child: Row(
        children: [
          const AppIcon('clock', size: 17),
          const SizedBox(width: 7),
          Expanded(
            child: Text(
              '${context.tr('queueShort')}  ${objects(queue['items']).length}',
            ),
          ),
          if (queue['paused'] == true) Text(context.tr('pauseQueue')),
          const AppIcon('chevron', size: 14),
        ],
      ),
    ),
  );
}

class LiveQueue extends StatefulWidget {
  const LiveQueue({
    super.key,
    required this.view,
    required this.session,
    required this.command,
    this.delegated = false,
    this.mutate,
    this.isPending,
    this.check,
  });
  final ValueNotifier<Map<String, dynamic>> view;
  final String session;
  final ConversationCommand command;
  final bool delegated;
  final Future<bool> Function(String, Map<String, dynamic>)? mutate;
  final bool Function()? isPending;
  final Future<bool> Function()? check;
  @override
  State<LiveQueue> createState() => _LiveQueueState();
}

class _LiveQueueState extends State<LiveQueue> {
  bool _busy = false;

  bool get _pending => widget.isPending?.call() == true;

  Future<bool> _mutate(String kind, Map<String, dynamic> data) async {
    if (widget.mutate != null) return widget.mutate!(kind, data);
    await widget.command(kind, data);
    return true;
  }

  Future<void> _run(String kind, Map<String, dynamic> data) async {
    if (_busy) return;
    setState(() => _busy = true);
    try {
      await _mutate(kind, data);
    } catch (error) {
      if (mounted) {
        showToast(context, failureLabel(error, translate: context.tr));
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _check() async {
    if (_busy || widget.check == null) return;
    setState(() => _busy = true);
    try {
      await widget.check!();
    } catch (error) {
      if (mounted) {
        showToast(context, failureLabel(error, translate: context.tr));
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _open(Map<String, dynamic> item) async {
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
        context.tr(widget.delegated ? 'queue' : 'edit'),
        // Child input keeps its captured owner; only reading and removal are
        // available here, including guidance retained after a stop or restart.
        child: widget.delegated
            ? SelectableText(draft)
            : StatefulBuilder(
                builder: (context, update) {
                  return Column(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      TextFormField(
                        initialValue: draft,
                        enabled: !pending && !_pending,
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
                                  final accepted = _pending
                                      ? await widget.check!()
                                      : await _mutate('edit_queued_turn', {
                                          'turn': item['turn'],
                                          'expected_revision':
                                              message['revision'],
                                          'message': {...input, 'text': draft},
                                        });
                                  if (context.mounted) {
                                    update(() => pending = false);
                                    if (accepted) Navigator.pop(context);
                                  }
                                } catch (error) {
                                  if (context.mounted) {
                                    update(() => pending = false);
                                    showToast(
                                      context,
                                      failureLabel(
                                        error,
                                        translate: context.tr,
                                      ),
                                    );
                                  }
                                }
                              },
                        child: Text(
                          context.tr(
                            _pending ? 'conversationCheckResult' : 'save',
                          ),
                        ),
                      ),
                    ],
                  );
                },
              ),
      );
    } catch (error) {
      if (mounted) {
        showToast(context, failureLabel(error, translate: context.tr));
      }
    }
    if (mounted) setState(() {});
  }

  @override
  Widget build(BuildContext context) => ValueListenableBuilder(
    valueListenable: widget.view,
    builder: (context, view, _) {
      final queue = object(object(object(view['snapshot'])['page'])['queue']);
      final items = objects(queue['items']);
      final enabled = !_busy && !_pending && view['connected'] == true;
      return Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (_pending && widget.check != null)
            OutlinedButton(
              onPressed: !_busy && view['connected'] == true ? _check : null,
              child: Text(context.tr('conversationCheckResult')),
            ),
          if (items.isEmpty)
            EmptyState(icon: 'chat', message: context.tr('queueEmpty')),
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
                      onTap: enabled && item['notice'] != true
                          ? () => _open(item)
                          : null,
                      child: Text(
                        item['notice'] == true
                            ? context.tr('backgroundNoticeTitle')
                            : item['preview'] as String? ?? '',
                        style: const TextStyle(height: 1.5),
                      ),
                    ),
                    Wrap(
                      children: [
                        if (!widget.delegated && item['notice'] != true)
                          TextButton(
                            onPressed: enabled ? () => _open(item) : null,
                            child: Text(context.tr('edit')),
                          ),
                        TextButton(
                          onPressed: enabled
                              ? () => _run('remove_queued_turn', {
                                  'turn': item['turn'],
                                  'expected_revision': item['revision'],
                                })
                              : null,
                          child: Text(context.tr('delete')),
                        ),
                        if (!widget.delegated &&
                            index > 0 &&
                            item['notice'] != true &&
                            items[index - 1]['notice'] != true)
                          TextButton(
                            onPressed: enabled
                                ? () => _run('move_queued_turn', {
                                    'session': widget.session,
                                    'expected_revision': queue['revision'],
                                    'turn': item['turn'],
                                    'before': items[index - 1]['turn'],
                                  })
                                : null,
                            child: Text(context.tr('moveUp')),
                          ),
                        if (!widget.delegated && item['notice'] != true)
                          TextButton(
                            onPressed: enabled
                                ? () => _run('send_queued_turn', {
                                    'turn': item['turn'],
                                    'expected_revision': item['revision'],
                                  })
                                : null,
                            child: Text(context.tr('send')),
                          ),
                      ],
                    ),
                  ],
                ),
              ),
            ),
          if (!widget.delegated && items.isNotEmpty)
            OutlinedButton(
              onPressed: enabled
                  ? () => _run('set_queue_paused', {
                      'session': widget.session,
                      'expected_revision': queue['revision'],
                      'paused': queue['paused'] != true,
                    })
                  : null,
              child: Text(
                context.tr(
                  queue['paused'] == true ? 'resumeQueue' : 'pauseQueue',
                ),
              ),
            ),
        ],
      );
    },
  );
}
