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
import 'turn_draft.dart';
import 'queue.dart';
import 'children.dart';
import 'presentation.dart';
import 'timeline.dart';
import 'background_data.dart';
import 'statistics.dart';
import 'voice.dart';
import 'message_data.dart';
import 'message_editor.dart';
import 'search.dart';
import 'turn_frame.dart';
import '../../resources/artifact/submission.dart';

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
  late final _submission = TurnDraft(
    host: widget.host,
    session: widget.sessionId,
    text: widget.initialDraft,
    attachments: widget.initialAttachments,
  );
  TextEditingController get _draft => _submission.input;
  List<PickedAttachment> get _attachments => _submission.attachments;
  bool get _sending => _submission.sending;
  PendingMessage? get _pendingSend => _submission.pending;
  final _scroll = ScrollController();
  final _view = ValueNotifier<Map<String, dynamic>>({});
  ConversationUpdates? _updates;
  bool _closed = false;
  bool _initialSubmitted = false;
  bool _historyBusy = false;
  ({String kind, String request})? _historyPending;
  Map<String, dynamic>? _backup;
  final _edits = <String, String>{};
  final _turnKeys = <String, GlobalKey>{};
  ({String turn, int revision})? _reveal;
  late final _artifactActions = ArtifactActions(
    submit: _sendArtifact,
    check: _checkSend,
    writable: () =>
        mounted &&
        widget.host.connected &&
        _view.value['connected'] == true &&
        !_historyBusy &&
        _historyPending == null &&
        !_sending &&
        _pendingSend == null &&
        _session['archived'] != true &&
        _session['delegation'] == null,
    pending: () => _pendingSend != null,
  );
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
    _submission.addListener(_rebuild);
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
      if (mounted) {
        showToast(context, failureLabel(error, translate: context.tr));
      }
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
        if (_closed || !mounted) break;
        final following =
            !_scroll.hasClients || _scroll.position.extentBefore < 60;
        _error = null;
        _view.value = value;
        if (value['removed'] == true) {
          final route = ModalRoute.of(context);
          if (route != null) Navigator.of(context).removeRoute(route);
          break;
        }
        _artifactActions.changed();
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

  void _rebuild() {
    setState(() {});
    _artifactActions.changed();
  }

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
    _submission.removeListener(_rebuild);
    _submission.dispose();
    _scroll.dispose();
    _artifactActions.dispose();
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
    showToast(context, failureLabel(error, translate: context.tr));
  }

  Future<void> _send() async {
    if (_historyBusy || _historyPending != null) return;
    try {
      await _submission.send(_session['revision'] as int);
    } catch (error) {
      _failure(error);
    }
  }

  Future<bool> _checkSend() async {
    try {
      return await _submission.check();
    } catch (error) {
      _failure(error);
      return false;
    }
  }

  Future<void> _sendArtifact(String message) async {
    if (!_artifactActions.writable()) throw const CommandFailure('unavailable');
    await _submission.send(_session['revision'] as int, message: message);
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
    context.tr('queue'),
    child: LiveQueue(
      view: _view,
      session: widget.sessionId,
      command: _command,
      delegated: _session['delegation'] != null,
      mutate: _historyCommand,
      isPending: () => _historyPending != null,
      check: () async {
        final pending = _historyPending;
        if (pending == null) return true;
        return _historyCommand(pending.kind, {});
      },
    ),
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

  void _openChild(Map<String, dynamic> reference) {
    final session = childSession(widget.host, reference);
    if (session == null) {
      _failure(context.tr('conversationUnavailable'));
      return;
    }
    unawaited(
      pushPage(
        context,
        LiveConversationPage(
          host: widget.host,
          sessionId: session['id'] as String,
          initialSession: session,
        ),
      ),
    );
  }

  void _configuration() => showAppSheet(
    context,
    context.tr('modelPicker'),
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
          title: text(_session['title'], context.tr('chat')),
          actions: [
            Surface(
              key: const ValueKey('conversation-actions'),
              radius: 20,
              padding: const EdgeInsets.symmetric(horizontal: 4),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  IconButton(
                    key: const ValueKey('conversation-ports'),
                    style: actionStyle,
                    icon: const AppIcon('link', size: 18),
                    tooltip: context.tr('ports'),
                    onPressed: () => pushPage(
                      context,
                      PortsPage(host: widget.host, session: widget.sessionId),
                    ),
                  ),
                  IconButton(
                    key: const ValueKey('conversation-info'),
                    style: actionStyle,
                    icon: const AppIcon('info', size: 18),
                    tooltip: context.tr('conversationStats'),
                    onPressed: () => showAppSheet(
                      context,
                      context.tr('conversationStats'),
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
                    tooltip: context.tr('messageSearch'),
                    onPressed: connected ? _searchMessages : null,
                  ),
                  IconButton(
                    style: actionStyle,
                    icon: const AppIcon('folder', size: 18),
                    tooltip: context.tr('resources'),
                    onPressed: () => pushPage(
                      context,
                      ResourcesPage(
                        hostId: widget.host.id,
                        worktreeId: _session['worktree'] as String,
                      ),
                    ),
                  ),
                  if (!delegated)
                    IconButton(
                      key: const ValueKey('conversation-settings'),
                      style: actionStyle,
                      icon: const AppIcon('settings', size: 18),
                      tooltip: context.tr('modelPicker'),
                      onPressed: _configuration,
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
                    content: Text(context.tr('conversationUnknown')),
                    actions: [
                      TextButton(
                        onPressed: _historyBusy
                            ? null
                            : () => _historyCommand(_historyPending!.kind, {}),
                        child: Text(context.tr('messageCheck')),
                      ),
                    ],
                  ),
                if (_backup != null)
                  MaterialBanner(
                    content: Text(context.tr('messageHistoryUpdated')),
                    actions: [
                      TextButton(
                        onPressed: () => _openSession(_backup!),
                        child: Text(context.tr('messageBackup')),
                      ),
                      IconButton(
                        tooltip: context.tr('close'),
                        onPressed: () => setState(() => _backup = null),
                        icon: const AppIcon('close'),
                      ),
                    ],
                  ),
                if (_pendingSend != null)
                  MaterialBanner(
                    content: Text(context.tr('conversationUnknown')),
                    actions: [
                      TextButton(
                        onPressed: _sending || !connected ? null : _checkSend,
                        child: Text(context.tr('conversationCheckResult')),
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
                                  label: context.tr('conversationLoading'),
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
                                  message: context.tr(
                                    'conversationUnavailable',
                                  ),
                                  action: Column(
                                    mainAxisSize: MainAxisSize.min,
                                    children: [
                                      if (reason != null)
                                        ErrorDetails(reason: reason),
                                      if (!_watching)
                                        FilledButton(
                                          onPressed: _retry,
                                          child: Text(context.tr('retry')),
                                        ),
                                    ],
                                  ),
                                ),
                        )
                      : connected &&
                            objects(_page['entries']).isEmpty &&
                            objects(_page['runs']).isEmpty &&
                            pendingBackground(_page).isEmpty &&
                            objects(object(view['snapshot'])['drafts']).isEmpty
                      ? CustomScrollView(
                          slivers: [
                            SliverFillRemaining(
                              hasScrollBody: false,
                              child: EmptyState(
                                icon: 'chat',
                                message: context.tr('conversationEmpty'),
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
                                toolOutput: (entry, index) async {
                                  final updates = _updates;
                                  if (_closed || updates == null) return null;
                                  final output = jsonDecode(
                                    await updates.toolOutput(
                                      entry: entry,
                                      index: index,
                                    ),
                                  );
                                  return output == null ? null : object(output);
                                },
                                onOpenChild: _openChild,
                                onTurnAction: _canUseHistory
                                    ? _turnAction
                                    : null,
                                canChangeHistory: _canChangeHistory,
                                sending: _sending || _pendingSend != null,
                                artifactActions: _artifactActions,
                                artifactChanges: _view,
                                canRestore: () => _canChangeHistory,
                                turnKey: (turn) =>
                                    _turnKeys.putIfAbsent(turn, GlobalKey.new),
                              ),
                              if (view['older_error'] != null)
                                Text(
                                  object(view['older_error'])['message']
                                          as String? ??
                                      context.tr('conversationFailed'),
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
                                  child: Text(context.tr('conversationOlder')),
                                ),
                            ],
                          ),
                        ),
                ),
              ],
            ),
          ),
          composer: delegated
              ? Column(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    if (objects(queue['items']).isNotEmpty)
                      QueueSummary(queue: queue, onOpen: _queue),
                    Padding(
                      padding: EdgeInsets.fromLTRB(
                        20,
                        12,
                        20,
                        MediaQuery.paddingOf(context).bottom + 12,
                      ),
                      child: Row(
                        children: [
                          Expanded(
                            child: Text(
                              context.tr('conversationChildReadonly'),
                            ),
                          ),
                          if (_active.isNotEmpty)
                            FilledButton(
                              onPressed: connected ? _stop : null,
                              child: Text(context.tr('stop')),
                            ),
                        ],
                      ),
                    ),
                  ],
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
                      : QueueSummary(queue: queue, onOpen: _queue),
                ),
        );
      },
    );
  }
}
