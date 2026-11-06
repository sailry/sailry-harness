import 'package:flutter/material.dart';
import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/kit.dart';
import '../../../ui/toast.dart';
import 'glass_chip.dart';
import 'presentation.dart' show ConversationCommand, failureLabel;
import 'guidance.dart';
import 'turn_draft.dart';

/// The optimistic preview stays until the projection shows its admitted turn.
GuidancePreview? outgoingGuidance(
  GuidancePreview? preview,
  Map<String, dynamic> view,
) => preview == null || object(view['guidance']).containsKey(preview.turn)
    ? null
    : preview;

typedef QueueState = ({int count, bool paused, bool guiding});

/// Counts queued input with guidance still awaiting admission; [guiding]
/// marks input waiting to join the running turn.
QueueState summarize(Map<String, dynamic> view, GuidancePreview? outgoing) {
  final queue = object(object(object(view['snapshot'])['page'])['queue']);
  final items = objects(queue['items']);
  final guidance = object(view['guidance']);
  final preview = outgoingGuidance(outgoing, view);
  return (
    count: items.length + (preview == null ? 0 : 1),
    paused: queue['paused'] == true,
    guiding:
        preview != null ||
        items.any((item) => guidance[item['turn']] == 'pending'),
  );
}

/// Speaks the count and the icon-only pending-guidance mark.
String describe(QueueState state, Translator tr) {
  final queue = tr(
    state.paused ? 'queuePaused' : 'queueCount',
  ).replaceAll('{count}', '${state.count}');
  return state.guiding
      ? tr('queueGuidance').replaceAll('{queue}', queue)
      : queue;
}

/// Queue state in the same capsule as the activity above the composer.
class QueueChip extends StatelessWidget {
  const QueueChip({super.key, required this.state, required this.onOpen});

  final QueueState state;
  final VoidCallback onOpen;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final count = '${state.count}';
    return Align(
      alignment: AlignmentDirectional.centerStart,
      child: Semantics(
        label: describe(state, context.tr),
        button: true,
        onTap: onOpen,
        excludeSemantics: true,
        child: GlassChip(
          onTap: onOpen,
          child: Row(
            mainAxisSize: MainAxisSize.min,
            spacing: 6,
            children: [
              AppIcon('chat', size: 15, color: colors.onSurfaceVariant),
              if (state.paused)
                Text(context.tr('queuePaused').replaceAll('{count}', count))
              else ...[
                Text(context.tr('queueShort')),
                Text(count, style: TextStyle(color: colors.onSurface)),
              ],
              if (state.guiding) const GuidanceStatus(state: 'pending'),
            ],
          ),
        ),
      ),
    );
  }
}

/// Guidance awaiting admission has no queue identity to act on yet.
class PendingGuidance extends StatelessWidget {
  const PendingGuidance({
    super.key,
    required this.preview,
    required this.attachments,
  });

  final String preview;
  final int attachments;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.only(bottom: 10),
    child: Surface(
      radius: 16,
      padding: const EdgeInsets.all(12),
      child: Row(
        children: [
          Expanded(
            child: Text(
              preview.trim().isEmpty ? context.tr('attachment') : preview,
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
              style: const TextStyle(height: 1.5),
            ),
          ),
          if (attachments > 0) ...[
            const AppIcon('file', size: 14),
            Text('$attachments'),
          ],
          const SizedBox(width: 8),
          const GuidanceStatus(state: 'pending'),
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
    this.submission,
    this.delegated = false,
    this.mutate,
    this.isPending,
    this.check,
  });
  final ValueNotifier<Map<String, dynamic>> view;
  final String session;
  final ConversationCommand command;

  /// Lists its guidance until the queue projection admits it.
  final TurnDraft? submission;
  final bool delegated;
  final Future<bool> Function(String, Map<String, dynamic>)? mutate;
  final bool Function()? isPending;
  final Future<bool> Function()? check;
  @override
  State<LiveQueue> createState() => _LiveQueueState();
}

class _LiveQueueState extends State<LiveQueue> {
  bool _busy = false;
  String? _guiding;

  bool get _pending => widget.isPending?.call() == true;

  Future<bool> _mutate(String kind, Map<String, dynamic> data) async {
    if (widget.mutate != null) return widget.mutate!(kind, data);
    await widget.command(kind, data);
    return true;
  }

  Future<void> _run(String kind, Map<String, dynamic> data) async {
    if (_busy) return;
    setState(() {
      _busy = true;
      _guiding = kind == 'guide_queued_turn' ? data['turn'] as String? : null;
    });
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
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: Listenable.merge([widget.view, widget.submission]),
    builder: (context, _) {
      final view = widget.view.value;
      final page = object(object(view['snapshot'])['page']);
      final queue = object(page['queue']);
      final recipient = runningRecipient(page);
      final guidance = object(view['guidance']);
      final items = objects(queue['items']);
      final outgoing = outgoingGuidance(widget.submission?.guidance, view);
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
          if (outgoing != null)
            PendingGuidance(
              preview: outgoing.text,
              attachments: outgoing.attachments.length,
            ),
          if (items.isEmpty && outgoing == null)
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
                      onTap:
                          enabled &&
                              item['notice'] != true &&
                              item['guidance'] == null
                          ? () => _open(item)
                          : null,
                      child: Text(
                        item['notice'] == true
                            ? context.tr('backgroundNoticeTitle')
                            : item['preview'] as String? ?? '',
                        style: const TextStyle(height: 1.5),
                      ),
                    ),
                    if (guidance[item['turn']] != null ||
                        (_busy || _pending) && _guiding == item['turn'])
                      GuidanceStatus(
                        state: (_busy || _pending) && _guiding == item['turn']
                            ? 'pending'
                            : guidance[item['turn']],
                      ),
                    Wrap(
                      children: [
                        if (!widget.delegated &&
                            item['notice'] != true &&
                            item['guidance'] == null)
                          TextButton(
                            onPressed: enabled ? () => _open(item) : null,
                            child: Text(context.tr('edit')),
                          ),
                        TextButton(
                          onPressed: enabled
                              ? () => _run(
                                  item['guidance'] == null
                                      ? 'remove_queued_turn'
                                      : 'withdraw_guidance',
                                  {
                                    'turn': item['turn'],
                                    'expected_revision': item['revision'],
                                  },
                                )
                              : null,
                          child: Text(
                            context.tr(
                              item['guidance'] == null
                                  ? 'delete'
                                  : 'guidanceWithdraw',
                            ),
                          ),
                        ),
                        if (!widget.delegated &&
                            index > 0 &&
                            item['notice'] != true &&
                            item['guidance'] == null &&
                            items[index - 1]['notice'] != true &&
                            items[index - 1]['guidance'] == null)
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
                            onPressed:
                                enabled &&
                                    (item['guidance'] == null ||
                                        item['guidance'] == recipient?['turn'])
                                ? () => _run(
                                    recipient != null && item['kind'] == 'task'
                                        ? 'guide_queued_turn'
                                        : 'send_queued_turn',
                                    {
                                      'turn': item['turn'],
                                      if (recipient != null &&
                                          item['kind'] == 'task')
                                        'recipient': recipient['turn'],
                                      'expected_revision': item['revision'],
                                    },
                                  )
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
