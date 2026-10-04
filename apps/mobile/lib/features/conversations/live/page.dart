import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:sailry_bridge/api/conversation.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../ui/kit.dart';
import '../../../ui/toast.dart';
import '../../resources/resources_page.dart';
import '../../resources/ports_page.dart';
import '../message_composer.dart';
import '../conversation_frame.dart';
import 'attachments.dart';
import 'configuration.dart';
import 'connection_status.dart';
import 'draft_attachments.dart';
import 'queue.dart';
import 'presentation.dart';
import 'timeline.dart';
import 'statistics.dart';
import 'voice.dart';
import 'message_data.dart';
import 'message_editor.dart';
import 'search.dart';
import 'turn_frame.dart';

part 'history_actions.dart';

class LiveConversationPage extends StatefulWidget {
  const LiveConversationPage({
    super.key,
    required this.host,
    required this.sessionId,
    required this.initialSession,
    this.initialDraft = '',
    this.initialAttachments = const [],
    this.submitInitial = false,
  });
  final HostConnection host;
  final String sessionId;
  final Map<String, dynamic> initialSession;
  final String initialDraft;
  final List<PickedAttachment> initialAttachments;
  final bool submitInitial;

  @override
  State<LiveConversationPage> createState() => _LiveConversationPageState();
}

class _LiveConversationPageState extends State<LiveConversationPage> {
  late final _draft = TextEditingController(text: widget.initialDraft);
  final _scroll = ScrollController();
  final _view = ValueNotifier<Map<String, dynamic>>({});
  late final List<PickedAttachment> _attachments = List.of(
    widget.initialAttachments,
  );
  ConversationUpdates? _updates;
  bool _closed = false;
  bool _sending = false;
  bool _initialSubmitted = false;
  bool _historyBusy = false;
  ({String kind, String request})? _historyPending;
  Map<String, dynamic>? _backup;
  final _edits = <String, String>{};
  final _turnKeys = <String, GlobalKey>{};
  ({String turn, int revision})? _reveal;
  ({String request, String text, List<PickedAttachment> attachments})?
  _pendingSend;
  Object? _error;
  bool _watching = false;

  Map<String, dynamic> get _session =>
      objects(
        widget.host.snapshot['sessions'],
      ).where((item) => item['id'] == widget.sessionId).firstOrNull ??
      widget.initialSession;
  Map<String, dynamic> get _snapshot => object(_view.value['snapshot']);
  Map<String, dynamic> get _page => object(_snapshot['page']);
  List<Map<String, dynamic>> get _active => objects(
    _page['runs'],
  ).where((run) => ['running', 'stopping'].contains(run['status'])).toList();

  @override
  void initState() {
    super.initState();
    unawaited(_acknowledge());
    unawaited(_watch());
  }

  Future<void> _acknowledge() async {
    final attention = object(object(_session['activity'])['attention']);
    if (attention['unread'] != true) return;
    try {
      await widget.host.command('set_session_read', {
        'session': widget.sessionId,
        'expected_revision': attention['revision'],
        'read': true,
      });
    } catch (error) {
      if (mounted) showToast(context, failureLabel(error));
    }
  }

  Future<void> _watch() async {
    if (_closed || _watching) return;
    _watching = true;
    ConversationUpdates? updates;
    try {
      updates = await widget.host.connection.watchConversation(
        session: widget.sessionId,
      );
      if (_closed) return;
      _updates = updates;
      while (!_closed) {
        final value = object(jsonDecode(await updates.next()));
        if (_closed) break;
        final following =
            !_scroll.hasClients || _scroll.position.extentBefore < 60;
        _error = null;
        _view.value = value;
        _turnKeys.removeWhere(
          (turn, _) =>
              !objects(_page['runs']).any((run) => run['turn'] == turn),
        );
        _revealMessage();
        if (widget.submitInitial &&
            !_initialSubmitted &&
            value['connected'] == true) {
          _initialSubmitted = true;
          unawaited(_send());
        }
        if (following && _reveal == null) {
          WidgetsBinding.instance.addPostFrameCallback((_) {
            if (mounted && _scroll.hasClients) {
              _scroll.jumpTo(_scroll.position.minScrollExtent);
            }
          });
        }
      }
    } catch (error) {
      if (!_closed && mounted) {
        setState(() => _error = error);
      }
    } finally {
      _watching = false;
      _updates = null;
      if (updates != null) {
        await updates.close();
        updates.dispose();
      }
    }
  }

  void _rebuild() => setState(() {});

  void _retry() {
    if (!widget.host.connected && widget.host.error != null) {
      unawaited(widget.host.watch());
    }
    if (_watching) return;
    unawaited(_watch());
    setState(() {});
  }

  @override
  void dispose() {
    _closed = true;
    final updates = _updates;
    if (updates != null) unawaited(updates.close());
    _draft.dispose();
    _scroll.dispose();
    _view.dispose();
    super.dispose();
  }

  Future<Map<String, dynamic>> _command(
    String kind,
    Map<String, dynamic> data,
  ) async {
    final result = await widget.host.command(kind, data);
    return result;
  }

  void _failure(Object error) {
    if (!mounted) return;
    showToast(context, failureLabel(error));
  }

  Future<void> _send() async {
    final text = _draft.text;
    if (_historyBusy ||
        _historyPending != null ||
        _sending ||
        _pendingSend != null ||
        !widget.host.connected ||
        text.trim().isEmpty && _attachments.isEmpty) {
      return;
    }
    final attachments = List<PickedAttachment>.of(_attachments);
    setState(() => _sending = true);
    try {
      await _command('submit_turn', {
        'session': widget.sessionId,
        'expected_revision': _session['revision'],
        'message': {
          'text': text,
          'attachments': attachments
              .map((item) => item.attachment['id'])
              .toList(),
        },
      });
      if (!mounted) return;
      // Preserve typing and newly attached files made while admission was pending.
      if (_draft.text == text) _draft.clear();
      setState(
        () => _attachments.removeWhere((item) => attachments.contains(item)),
      );
    } catch (error) {
      if (error is CommandFailure &&
          error.code == 'outcome_unknown' &&
          error.request != null &&
          mounted) {
        setState(
          () => _pendingSend = (
            request: error.request!,
            text: text,
            attachments: attachments,
          ),
        );
      }
      _failure(error);
    } finally {
      if (mounted) setState(() => _sending = false);
    }
  }

  Future<void> _checkSend() async {
    final pending = _pendingSend;
    if (pending == null || _sending) return;
    setState(() => _sending = true);
    try {
      // Explicit reconciliation keeps the original durable request identity.
      await widget.host.execute(pending.request);
      if (!mounted) return;
      if (_draft.text == pending.text) _draft.clear();
      setState(() {
        _attachments.removeWhere(pending.attachments.contains);
        _pendingSend = null;
      });
    } catch (error) {
      _failure(error);
    } finally {
      if (mounted) setState(() => _sending = false);
    }
  }

  Future<void> _stop() async {
    try {
      for (final run in _active) {
        await _command('stop_turn', {'turn': run['turn']});
      }
    } catch (error) {
      _failure(error);
    }
  }

  void _queue() => showAppSheet(
    context,
    tr('queue'),
    child: LiveQueue(view: _view, session: widget.sessionId, command: _command),
  );

  Future<void> _attach() async {
    try {
      final attachment = await pickAttachment(
        widget.host,
        _session['worktree'] as String,
      );
      if (mounted && attachment != null) {
        setState(() => _attachments.add(attachment));
      }
    } catch (error) {
      _failure(error);
    }
  }

  Future<void> _voice() => dictate(context, _draft);

  void _openChild(Map<String, dynamic> child) {
    final id = object(child['run'])['session'];
    final session = objects(widget.host.snapshot['sessions'])
        .where(
          (session) =>
              session['id'] == id &&
              session['worktree'] == _session['worktree'] &&
              session['delegation'] != null,
        )
        .firstOrNull;
    if (session == null) {
      _failure(tr('conversationUnavailable'));
      return;
    }
    unawaited(
      pushPage(
        context,
        LiveConversationPage(
          host: widget.host,
          sessionId: id as String,
          initialSession: session,
        ),
      ),
    );
  }

  void _configuration() => showAppSheet(
    context,
    tr('modelPicker'),
    child: ConversationConfiguration(host: widget.host, session: _session),
  );

  @override
  Widget build(BuildContext context) {
    AppSession.maybeOf(context);
    return ValueListenableBuilder(
      valueListenable: _view,
      builder: (context, view, _) {
        final connected =
            widget.host.connected &&
            view['connected'] == true &&
            _error == null;
        final queue = object(_page['queue']);
        final delegated = _session['delegation'] != null;
        final fault = object(view['error']);
        final hostFault = object(widget.host.error);
        final faults = [fault, hostFault].where((error) => error.isNotEmpty);
        final displayed =
            faults
                .where(
                  (error) => !['unavailable', 'busy'].contains(error['code']),
                )
                .firstOrNull ??
            faults.firstOrNull;
        final reason =
            displayed?['message'] as String? ??
            _error?.toString() ??
            hostFault['message'] as String? ??
            widget.host.error?.toString();
        // These are the observer's retryable faults, not an application retry loop.
        final reconnecting =
            _error == null &&
            (widget.host.error == null || hostFault.isNotEmpty) &&
            [fault, hostFault].every(
              (error) =>
                  error.isEmpty ||
                  ['unavailable', 'busy'].contains(error['code']),
            );
        final actionStyle = IconButton.styleFrom(
          fixedSize: const Size.square(36),
          minimumSize: const Size.square(36),
          padding: EdgeInsets.zero,
          tapTargetSize: MaterialTapTargetSize.shrinkWrap,
        );
        final loading =
            _snapshot.isEmpty && reason == null && widget.host.connected;
        final unavailable = !connected;
        final hasBanner =
            _historyPending != null ||
            _backup != null ||
            _pendingSend != null ||
            unavailable && _snapshot.isNotEmpty;
        return ConversationFrame(
          title: text(_session['title'], tr('chat')),
          leading: delegated
              ? null
              : Align(
                  alignment: Alignment.centerLeft,
                  child: Surface(
                    radius: 20,
                    padding: EdgeInsets.zero,
                    child: IconButton(
                      style: actionStyle,
                      icon: const AppIcon('settings', size: 18),
                      tooltip: tr('modelPicker'),
                      onPressed: _configuration,
                    ),
                  ),
                ),
          actions: [
            Surface(
              radius: 20,
              padding: const EdgeInsets.symmetric(horizontal: 4),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  IconButton(
                    key: const ValueKey('conversation-ports'),
                    style: actionStyle,
                    icon: const AppIcon('link', size: 18),
                    tooltip: tr('ports'),
                    onPressed: () => pushPage(
                      context,
                      PortsPage(host: widget.host, session: widget.sessionId),
                    ),
                  ),
                  IconButton(
                    key: const ValueKey('conversation-info'),
                    style: actionStyle,
                    icon: const AppIcon('info', size: 18),
                    tooltip: tr('conversationStats'),
                    onPressed: () => showAppSheet(
                      context,
                      tr('conversationStats'),
                      child: ValueListenableBuilder(
                        valueListenable: _view,
                        builder: (context, view, _) => ConversationStatistics(
                          data: object(object(view['snapshot'])['statistics']),
                        ),
                      ),
                    ),
                  ),
                  IconButton(
                    style: actionStyle,
                    icon: const AppIcon('search', size: 18),
                    tooltip: tr('messageSearch'),
                    onPressed: connected ? _searchMessages : null,
                  ),
                  IconButton(
                    style: actionStyle,
                    icon: const AppIcon('folder', size: 18),
                    tooltip: tr('resources'),
                    onPressed: () => pushPage(
                      context,
                      ResourcesPage(
                        hostId: widget.host.id,
                        worktreeId: _session['worktree'] as String,
                      ),
                    ),
                  ),
                ],
              ),
            ),
          ],
          bodyBuilder: (context, bottomInset) => LayoutBuilder(
            builder: (context, constraints) => Column(
              children: [
                if (hasBanner)
                  SizedBox(height: ConversationFrame.contentInset(context)),
                if (_historyPending != null)
                  MaterialBanner(
                    content: Text(tr('conversationUnknown')),
                    actions: [
                      TextButton(
                        onPressed: _historyBusy
                            ? null
                            : () => _historyCommand(_historyPending!.kind, {}),
                        child: Text(tr('messageCheck')),
                      ),
                    ],
                  ),
                if (_backup != null)
                  MaterialBanner(
                    content: Text(tr('messageHistoryUpdated')),
                    actions: [
                      TextButton(
                        onPressed: () => _openSession(_backup!),
                        child: Text(tr('messageBackup')),
                      ),
                      IconButton(
                        tooltip: tr('close'),
                        onPressed: () => setState(() => _backup = null),
                        icon: const AppIcon('close'),
                      ),
                    ],
                  ),
                if (_pendingSend != null)
                  MaterialBanner(
                    content: Text(tr('conversationUnknown')),
                    actions: [
                      TextButton(
                        onPressed: _sending || !connected ? null : _checkSend,
                        child: Text(tr('conversationCheckResult')),
                      ),
                    ],
                  ),
                if (unavailable && _snapshot.isNotEmpty)
                  ConstrainedBox(
                    constraints: BoxConstraints(
                      maxHeight:
                          ((constraints.maxHeight -
                                      bottomInset -
                                      ConversationFrame.contentInset(context)) /
                                  2)
                              .clamp(0, 240),
                    ),
                    child: SingleChildScrollView(
                      key: const PageStorageKey('connection-status-scroll'),
                      padding: const EdgeInsets.symmetric(horizontal: 20),
                      child: ConnectionStatus(
                        reconnecting: reconnecting,
                        reason: reason,
                        onRetry: reconnecting ? null : _retry,
                      ),
                    ),
                  ),
                Expanded(
                  child: unavailable && _snapshot.isEmpty
                      ? Padding(
                          padding: EdgeInsets.only(
                            top: ConversationFrame.contentInset(context),
                            bottom: bottomInset,
                          ),
                          child: loading
                              ? LoadingOverlay(
                                  loading: true,
                                  label: tr('conversationLoading'),
                                  child: const SizedBox.expand(),
                                )
                              : reconnecting
                              ? Center(
                                  child: SingleChildScrollView(
                                    padding: const EdgeInsets.all(24),
                                    child: ConnectionStatus(
                                      reconnecting: true,
                                      reason: reason,
                                    ),
                                  ),
                                )
                              : FailureState(
                                  icon: 'server',
                                  message: tr('conversationUnavailable'),
                                  action: Column(
                                    mainAxisSize: MainAxisSize.min,
                                    children: [
                                      if (reason != null)
                                        ErrorDetails(reason: reason),
                                      if (!_watching)
                                        FilledButton(
                                          onPressed: _retry,
                                          child: Text(tr('retry')),
                                        ),
                                    ],
                                  ),
                                ),
                        )
                      : connected &&
                            objects(_page['entries']).isEmpty &&
                            objects(_page['runs']).isEmpty &&
                            objects(object(view['snapshot'])['drafts']).isEmpty
                      ? CustomScrollView(
                          slivers: [
                            SliverFillRemaining(
                              hasScrollBody: false,
                              child: EmptyState(
                                icon: 'chat',
                                message: tr('conversationEmpty'),
                              ),
                            ),
                          ],
                        )
                      : Align(
                          alignment: Alignment.topCenter,
                          child: ListView(
                            shrinkWrap: true,
                            controller: _scroll,
                            reverse: true,
                            keyboardDismissBehavior:
                                ScrollViewKeyboardDismissBehavior.onDrag,
                            padding: EdgeInsets.fromLTRB(
                              20,
                              hasBanner
                                  ? 0
                                  : ConversationFrame.contentInset(context),
                              20,
                              bottomInset + 8,
                            ),
                            children: [
                              LiveTimeline(
                                key: ValueKey((
                                  widget.host.id,
                                  widget.sessionId,
                                )),
                                view: view,
                                session: _session,
                                host: widget.host,
                                command: _command,
                                onOpenChild: _openChild,
                                onTurnAction: _canUseHistory
                                    ? _turnAction
                                    : null,
                                canChangeHistory: _canChangeHistory,
                                sending: _sending || _pendingSend != null,
                                canRestore: () => _canChangeHistory,
                                turnKey: (turn) =>
                                    _turnKeys.putIfAbsent(turn, GlobalKey.new),
                              ),
                              if (view['older_error'] != null)
                                Text(
                                  object(view['older_error'])['message']
                                          as String? ??
                                      tr('conversationFailed'),
                                ),
                              if (_page['next_before'] != null)
                                TextButton(
                                  onPressed: view['loading_older'] == true
                                      ? null
                                      : () async {
                                          try {
                                            await _updates?.loadOlder();
                                          } catch (error) {
                                            _failure(error);
                                          }
                                        },
                                  child: Text(tr('conversationOlder')),
                                ),
                            ],
                          ),
                        ),
                ),
              ],
            ),
          ),
          composer: delegated
              ? Padding(
                  padding: EdgeInsets.fromLTRB(
                    20,
                    12,
                    20,
                    MediaQuery.paddingOf(context).bottom + 12,
                  ),
                  child: Row(
                    children: [
                      Expanded(child: Text(tr('conversationChildReadonly'))),
                      if (_active.isNotEmpty)
                        FilledButton(
                          onPressed: connected ? _stop : null,
                          child: Text(tr('stop')),
                        ),
                    ],
                  ),
                )
              : MessageComposer(
                  controller: _draft,
                  attachments: _attachments.isEmpty
                      ? null
                      : DraftAttachments(
                          attachments: _attachments,
                          onRemove: (attachment) =>
                              setState(() => _attachments.remove(attachment)),
                        ),
                  attached: false,
                  busy: _active.isNotEmpty,
                  enabled:
                      connected &&
                      !_historyBusy &&
                      _historyPending == null &&
                      !_sending &&
                      _pendingSend == null &&
                      _session['archived'] != true,
                  onAttachment: (_) => _attach(),
                  onVoice: _voice,
                  onStop: _stop,
                  onSend: _send,
                  priority: objects(queue['items']).isEmpty
                      ? null
                      : Padding(
                          padding: const EdgeInsets.symmetric(horizontal: 14),
                          child: Surface(
                            radius: 18,
                            padding: const EdgeInsets.symmetric(
                              horizontal: 12,
                              vertical: 6,
                            ),
                            onTap: _queue,
                            child: Row(
                              children: [
                                const AppIcon('clock', size: 17),
                                const SizedBox(width: 7),
                                Expanded(
                                  child: Text(
                                    '${tr('queueShort')}  ${objects(queue['items']).length}',
                                  ),
                                ),
                                if (queue['paused'] == true)
                                  Text(tr('pauseQueue')),
                                const AppIcon('chevron', size: 14),
                              ],
                            ),
                          ),
                        ),
                ),
        );
      },
    );
  }
}
