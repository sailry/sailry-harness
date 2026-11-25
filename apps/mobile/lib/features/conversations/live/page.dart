import 'dart:async';
import 'dart:convert';
import 'dart:isolate';

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart' show ScrollDirection;
import 'package:sailry_bridge/api/conversation.dart';
import 'package:sailry_bridge/bridge.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../ui/connection_status.dart';
import '../../../ui/kit.dart';
import '../../../ui/loading.dart';
import '../../../ui/toast.dart';
import '../../resources/resources_page.dart';
import '../message_composer.dart';
import '../conversation_frame.dart';
import 'activity_strip.dart';
import 'attention.dart';
import 'attachments.dart';
import 'configuration.dart';
import 'connection_status.dart';
import 'draft_attachments.dart';
import 'glass_chip.dart' show ComposerChipRow;
import 'turn_draft.dart';
import 'queue.dart';
import 'guidance.dart';
import 'children.dart';
import 'history_write.dart';
import 'presentation.dart';
import 'tool_output.dart';
import 'timeline.dart';
import 'transcript_index.dart';
import 'statistics.dart';
import 'message_data.dart';
import 'message_editor.dart';
import 'session_links.dart';
import 'turn_frame.dart';
import '../../resources/artifact/submission.dart';

part 'history_actions.dart';

typedef _StatisticsState = ({Map<String, dynamic>? data, String? statusKey});

class LiveConversationPage extends StatefulWidget {
  const LiveConversationPage({
    super.key,
    required this.host,
    required this.sessionId,
    required this.initialSession,
  });
  final HostConnection host;
  final String sessionId;
  final Map<String, dynamic> initialSession;

  @override
  State<LiveConversationPage> createState() => _LiveConversationPageState();
}

class _LiveConversationPageState extends State<LiveConversationPage>
    with WidgetsBindingObserver {
  late final _submission = TurnDraft.retain(
    host: widget.host,
    session: widget.sessionId,
  );
  late final _history = HistoryWrite.retain(widget.host, widget.sessionId);
  TextEditingController get _draft => _submission.input;
  List<PickedAttachment> get _attachments => _submission.attachments;
  bool get _sending => _submission.sending;
  PendingMessage? get _pendingSend => _submission.pending;
  static const _followDistance = 60.0;
  final _scroll = ScrollController();
  double? _returnOffset;
  final _view = ValueNotifier<Map<String, dynamic>>({});
  ValueNotifier<_StatisticsState>? _statisticsSheet;
  ModalRoute<dynamic>? _statisticsRoute;
  var _transcript = TranscriptIndex(const {});
  Future<ConversationUpdates?>? _observation;
  Future<void> _released = Future.value();
  ConversationUpdates? _updates;
  final _toolSelection = ToolSelection();
  Future<void> _trackingTools = Future.value();
  bool _closed = false;
  bool get _historyBusy => _history.busy;
  String? get _historyPending => _history.pendingKind;
  Map<String, dynamic>? _backup;
  final _edits = <String, String>{};
  final _turnKeys = <String, GlobalKey>{};
  ({String turn, int revision})? _reveal;
  bool _seeking = false;
  late final _artifactActions = ArtifactActions(
    submit: _sendArtifact,
    check: _checkSend,
    writable: () =>
        mounted &&
        !_ownerEnded &&
        widget.host.connected &&
        _view.value['connected'] == true &&
        _error == null &&
        !_historyBusy &&
        _historyPending == null &&
        !_sending &&
        _pendingSend == null &&
        _session['archived'] != true &&
        _session['delegation'] == null,
    pending: () => _pendingSend != null,
  );
  Object? _error;
  bool get _watching => _observation != null;
  bool _waitingForHost = false;
  bool _retrying = false;
  bool _away = false;
  bool _current = false;
  bool _pushingConversation = false;
  bool _covered = false;
  bool _ownerEnded = false;
  AppSession? _directory;
  bool _registeredOwner = false;
  ModalRoute<dynamic>? _route;
  late final _attention = ConversationAttention(
    host: widget.host,
    sessionId: widget.sessionId,
    session: () => _session,
    active: () =>
        mounted &&
        !_closed &&
        !_ownerEnded &&
        !_away &&
        _route?.isCurrent != false,
  );
  Animation<double>? _entrance;

  /// Whether the push transition has finished. A fetched transcript is held
  /// until then so its first build never stalls the route animation; one
  /// already in memory renders with the route.
  bool _settled = false;

  /// The transcript is the last one seen, shown until the live observation
  /// replaces it. It never enables writes or reports a connection.
  bool _cached = false;

  /// The newest observed payload, persisted when the page leaves.
  String? _payload;

  String get _cacheKey =>
      '${widget.host.cacheScope}conversation/${widget.sessionId}';

  Map<String, dynamic> get _session {
    final sessions = widget.host.snapshot['sessions'];
    if (sessions is List) {
      for (final item in sessions) {
        if (item is Map && item['id'] == widget.sessionId) return object(item);
      }
    }
    return widget.initialSession;
  }

  Map<String, dynamic> get _snapshot => _transcript.snapshot;
  Map<String, dynamic> get _page => _transcript.page;
  List<Map<String, dynamic>> get _active => _transcript.active;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _scroll.addListener(_scrollChanged);
    _submission.addListener(_rebuild);
    _history.addListener(_rebuild);
    widget.host.addListener(_hostChanged);
    _restoreCached();
    unawaited(_watch());
  }

  void _restoreCached() {
    final cache = widget.host.cache;
    if (cache == null || _ownerEnded) return;
    final kept = cache.peek<TranscriptIndex>(_cacheKey);
    if (kept != null) {
      _settled = true;
      _showCached(kept);
      return;
    }
    unawaited(
      cache.read(_cacheKey, transcriptIndex).then((stored) {
        if (stored == null || !mounted || _covered || _ownerEnded) return;
        if (_snapshot.isNotEmpty) return;
        setState(() => _showCached(stored));
      }),
    );
  }

  void _showCached(TranscriptIndex transcript) {
    _cached = true;
    _transcript = transcript;
    _view.value = {
      ...transcript.view,
      'connected': false,
      'error': _view.value['error'],
      'statistics_error': null,
    };
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted && _cached) _restoreScroll(true);
    });
  }

  /// Keeps the newest transcript for the next visit, on disk as well when
  /// the page closes or the app is suspended.
  void _keep({required bool persist}) {
    final payload = _payload;
    final cache = widget.host.cache;
    if (payload == null || _ownerEnded || _cached || cache == null) return;
    cache.put(
      _cacheKey,
      _transcript,
      raw: persist ? payload : null,
      size: payload.length,
    );
    if (persist) {
      unawaited(
        cache.index(
          widget.host.cacheScope,
          widget.sessionId,
          'transcript',
          messageBodies(objects(_page['entries'])),
        ),
      );
    }
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final directory = context
        .getInheritedWidgetOfExactType<SessionScope>()
        ?.notifier;
    if (!identical(directory, _directory)) {
      _directory?.removeListener(_directoryChanged);
      _directory = directory;
      directory?.addListener(_directoryChanged);
      _directoryChanged();
    }
    if (_terminalFault(widget.host.error)) _endOwner(widget.host.error);
    final route = ModalRoute.of(context);
    _route = route;
    final current = route?.isCurrent ?? true;
    if (_current != current) {
      _current = current;
      _acknowledge();
    }
    if (_settled) return;
    final animation = route?.animation;
    if (identical(animation, _entrance)) return;
    _entrance?.removeStatusListener(_entered);
    _entrance = animation;
    if (animation == null || animation.isCompleted) {
      _settled = true;
    } else {
      animation.addStatusListener(_entered);
    }
  }

  void _entered(AnimationStatus status) {
    if (!status.isCompleted || !mounted) return;
    _entrance?.removeStatusListener(_entered);
    _entrance = null;
    setState(() => _settled = true);
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.paused ||
        state == AppLifecycleState.hidden) {
      _away = true;
      _keep(persist: true);
    } else if (state == AppLifecycleState.resumed && _away) {
      _away = false;
      _acknowledge();
      // The stream can stall in the background; resubscribe for a fresh view.
      if (!_covered && !_ownerEnded && !_closed) {
        _stopWatching();
        unawaited(_watch());
      }
    }
  }

  void _acknowledge() => unawaited(_attention.observe());

  void _hostChanged() {
    if (_closed || _ownerEnded) return;
    if (_terminalFault(widget.host.error)) {
      _endOwner(widget.host.error);
      return;
    }
    _acknowledge();
    if (_waitingForHost && widget.host.hasConnection) unawaited(_watch());
    _updateStatistics();
    if (mounted) setState(() {});
  }

  void _directoryChanged() {
    final directory = _directory;
    if (directory == null) return;
    if (identical(directory.host(widget.host.id), widget.host)) {
      _registeredOwner = true;
    } else if (_registeredOwner) {
      _endOwner();
    }
  }

  static bool _terminalFault(Object? fault) {
    final code = fault is BridgeFailure ? fault.code : object(fault)['code'];
    // Local close first retires the observation, so its cancellation is ignored.
    return fault is BridgeFailure && code == 'cancelled' ||
        const {'permission_denied', 'not_found', 'expired'}.contains(code);
  }

  void _endOwner([Object? fault]) {
    if (_closed || _ownerEnded) return;
    _ownerEnded = true;
    _payload = null;
    _cached = false;
    final cache = widget.host.cache;
    if (cache != null) {
      unawaited(cache.removeScope(_cacheKey));
      for (final source in const ['preview', 'transcript']) {
        unawaited(
          cache.index(widget.host.cacheScope, widget.sessionId, source, []),
        );
      }
    }
    _error = fault;
    _returnOffset = null;
    _discardView();
    setState(() {});
  }

  void _discardView() {
    _dismissStatistics(_statisticsRoute);
    _stopWatching();
    _turnKeys.clear();
    _reveal = null;
    _transcript = TranscriptIndex(const {});
    _view.value = const {};
    _artifactActions.changed();
  }

  void _stopWatching() {
    final observation = _observation;
    if (observation == null) return;
    _observation = null;
    _updates = null;
    _released = _releaseObservation(observation);
  }

  static Future<void> _releaseObservation(
    Future<ConversationUpdates?> observation,
  ) async {
    final updates = await observation.then(
      (value) => value,
      onError: (Object _) => null,
    );
    if (updates == null) return;
    try {
      await updates.close();
    } catch (error) {
      debugPrint('Conversation observation cleanup failed: $error');
    } finally {
      updates.dispose();
    }
  }

  Future<void> _watch() async {
    if (_closed || _covered || _ownerEnded || _watching) return;
    _waitingForHost = !widget.host.hasConnection;
    if (_waitingForHost) return;
    // Reacquire after release, including a handle that was still opening.
    late final Future<ConversationUpdates?> observation;
    bool ownsObservation() => identical(_observation, observation);
    observation = _released.then<ConversationUpdates?>((_) {
      if (!ownsObservation()) return null;
      return widget.host.connection.watchConversation(
        session: widget.sessionId,
        referenceRows: true,
        recent:
            _reveal == null && (_returnOffset == null || _returnOffset == 0),
      );
    });
    _observation = observation;
    try {
      final updates = await observation;
      if (!mounted || !ownsObservation() || updates == null) return;
      _updates = updates;
      _toolSelection.reset();
      while (ownsObservation()) {
        final payload = await updates.next();
        if (!mounted || !ownsObservation()) return;
        final transcript = await _decode(payload);
        if (!mounted || !ownsObservation()) return;
        final value = transcript.view;
        final fault = value['error'];
        if (value['removed'] == true) {
          _endOwner(fault);
          final route = _route;
          if (route != null && route.isActive) {
            Navigator.of(context).removeRoute(route);
          }
          return;
        }
        if (_terminalFault(fault)) {
          _endOwner(fault);
          return;
        }
        _error = null;
        final live = value['connected'] == true;
        if (transcript.snapshot.isEmpty || (!live && _snapshot.isNotEmpty)) {
          // A recovering observation may deliver faults before the new view.
          _view.value = {..._view.value, 'connected': false, 'error': fault};
          _updateStatistics();
          _artifactActions.changed();
          continue;
        }
        final following =
            _returnOffset == null &&
            (!_scroll.hasClients ||
                (_scroll.position.extentBefore == 0 &&
                    !_scroll.position.isScrollingNotifier.value));
        _cached = !live;
        _payload = live ? payload : null;
        final statisticsError = value['statistics_error'];
        final previousStatisticsError = _view.value['statistics_error'];
        final detailErrors = newDetailErrors(_view.value, value).toList();
        _transcript = transcript;
        _keep(persist: false);
        _view.value = mergeToolDetails(_view.value, value);
        _updateStatistics();
        _submission.observe(object(_view.value['guidance']));
        if (_statisticsSheet == null &&
            statisticsError != null &&
            jsonEncode(statisticsError) !=
                jsonEncode(previousStatisticsError)) {
          showToast(
            context,
            '${context.tr('conversationStats')}: ${context.tr('settingsLoadFailed')}',
          );
        }
        for (final fault in detailErrors) {
          showToast(
            context,
            failureLabel(fault['message'], translate: context.tr),
          );
        }
        _artifactActions.changed();
        _turnKeys.removeWhere((turn, _) => !transcript.containsTurn(turn));
        _revealMessage();
        if ((following || _returnOffset != null) && _reveal == null) {
          WidgetsBinding.instance.addPostFrameCallback((_) {
            if (ownsObservation()) _restoreScroll(following);
          });
        }
      }
    } catch (error) {
      if (!mounted || !ownsObservation()) return;
      if (_terminalFault(error)) {
        _endOwner(error);
      } else {
        setState(() => _error = error);
        _updateStatistics();
        _artifactActions.changed();
      }
    } finally {
      if (ownsObservation()) _stopWatching();
    }
  }

  void _scrollChanged() {
    if (_scroll.hasClients &&
        _scroll.position.userScrollDirection != ScrollDirection.idle) {
      _returnOffset = null;
      _loadOlderNearTop(retry: true);
    }
  }

  bool get _hasOlder =>
      _page['next_before'] != null ||
      (_snapshot['missing'] as List? ?? const []).isNotEmpty;

  bool _loadingOlder = false;
  bool _olderFailed = false;

  /// Earlier history loads as the transcript nears its top, the end of the
  /// reversed list, including while it is shorter than the viewport. A failed
  /// load waits for the user to scroll again.
  void _loadOlderNearTop({bool retry = false}) {
    if (_loadingOlder ||
        !_hasOlder ||
        !_scroll.hasClients ||
        _view.value['loading_older'] == true ||
        (_olderFailed || _view.value['older_error'] != null) && !retry) {
      return;
    }
    final position = _scroll.position;
    if (position.extentAfter > position.viewportDimension) return;
    _loadingOlder = true;
    _olderFailed = false;
    unawaited(() async {
      try {
        await _updates?.loadOlder();
      } catch (error) {
        _olderFailed = true;
        if (mounted) _failure(error);
      } finally {
        _loadingOlder = false;
        if (mounted) {
          WidgetsBinding.instance.addPostFrameCallback(
            (_) => _loadOlderNearTop(),
          );
        }
      }
    }());
  }

  void _restoreScroll(bool following) {
    if (_reveal != null) return;
    if (!_scroll.hasClients) {
      if (_settled &&
          _view.value['connected'] == true &&
          _transcript.runs.isEmpty &&
          (_snapshot['drafts'] as List? ?? const []).isEmpty) {
        _returnOffset = null;
      }
      return;
    }
    final position = _scroll.position;
    if (position.isScrollingNotifier.value) return;
    final saved = _returnOffset;
    if (saved == null && !following) return;
    final desired = position.minScrollExtent + (saved ?? 0);
    final offset = desired
        .clamp(position.minScrollExtent, position.maxScrollExtent)
        .toDouble();
    if (offset != position.pixels) position.jumpTo(offset);
    // Client may still be recovering older pages after the first snapshot.
    if (desired == offset || !_hasOlder) {
      _returnOffset = null;
    }
  }

  /// Prepare the timeline lookups in the same isolate that parses the packet.
  static Future<TranscriptIndex> _decode(String payload) async {
    TranscriptIndex decode() =>
        TranscriptIndex(jsonDecode(payload) as Map<String, dynamic>);
    return payload.length < 32 * 1024 ? decode() : await Isolate.run(decode);
  }

  void _trackTools(Object owner, List<Map<String, dynamic>> calls, bool retry) {
    final selection = _toolSelection.update(owner, calls, retry: retry);
    final updates = _updates;
    if (_closed || updates == null || selection == null) return;
    _trackingTools = _trackingTools.then((_) async {
      if (_closed || !identical(_updates, updates)) return;
      try {
        await updates.trackTools(selection: selection);
      } catch (error) {
        if (mounted && identical(_updates, updates)) {
          showToast(context, failureLabel(error, translate: context.tr));
        }
      }
    });
  }

  void _rebuild() {
    // Admission may complete after the projection already showed the turn.
    _submission.observe(object(_view.value['guidance']));
    setState(() {});
    _artifactActions.changed();
  }

  Future<void> _retry() async {
    if (_closed || _covered || _ownerEnded || _retrying) return;
    if (!widget.host.connected && widget.host.error != null) {
      unawaited(widget.host.watch());
    }
    if (_updates == null) {
      if (!_watching) {
        unawaited(_watch());
        setState(() {});
      }
      return;
    }
    setState(() => _retrying = true);
    try {
      await widget.host.connection.refreshConversation(
        session: widget.sessionId,
      );
    } catch (error) {
      _failure(error);
    } finally {
      if (mounted) setState(() => _retrying = false);
    }
  }

  @override
  void dispose() {
    _keep(persist: true);
    _closed = true;
    _dismissStatistics(_statisticsRoute);
    WidgetsBinding.instance.removeObserver(this);
    widget.host.removeListener(_hostChanged);
    _directory?.removeListener(_directoryChanged);
    _attention.dispose();
    _entrance?.removeStatusListener(_entered);
    _stopWatching();
    _submission.removeListener(_rebuild);
    _submission.release();
    _history.removeListener(_rebuild);
    _history.release();
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
    final recipient = runningRecipient(_page);
    try {
      await _submission.send(
        _session['revision'] as int,
        delegated: _session['delegation'] != null,
        recipient: recipient == null
            ? null
            : (
                turn: recipient['turn'] as String,
                revision: recipient['revision'] as int,
              ),
      );
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

  String? _pendingPlanFeedback(String question) =>
      _pendingSend?.question == question ? _pendingSend?.text : null;

  Future<void> _sendPlanFeedback(String question, String message) async {
    final pending = _pendingSend;
    if (pending != null) {
      if (pending.question != question || !await _submission.check()) {
        throw const CommandFailure('unavailable');
      }
      return;
    }
    if (!_artifactActions.writable() ||
        object(_session['config'])['mode'] != 'plan' ||
        !objects(_page['questions']).any(
          (item) =>
              item['id'] == question &&
              object(item['state'])['kind'] == 'pending',
        )) {
      throw const CommandFailure('conflict');
    }
    // New input atomically supersedes the completed review in the Node.
    await _submission.send(
      _session['revision'] as int,
      message: message,
      question: question,
    );
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
      submission: _submission,
      delegated: _session['delegation'] != null,
      mutate: _historyCommand,
      isPending: () => _historyPending != null,
      check: _checkHistory,
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

  void _openChild(Map<String, dynamic> reference) {
    final session = childSession(widget.host, reference);
    if (session == null) {
      _failure(context.tr('conversationUnavailable'));
      return;
    }
    unawaited(_openSession(session));
  }

  Future<void> _openSession(Map<String, dynamic> session) async {
    final id = session['id'];
    if (id is! String || _closed || _ownerEnded || _pushingConversation) return;
    _pushingConversation = true;
    final page = LiveConversationPage(
      host: widget.host,
      sessionId: id,
      initialSession: session,
    );
    PageRoute<dynamic>? covering;
    try {
      await pushPage<void>(
        context,
        Builder(
          builder: (context) {
            final route = ModalRoute.of(context);
            if (covering == null &&
                route is PageRoute<dynamic> &&
                route.opaque) {
              covering = route;
              _observeCover(route);
            }
            return page;
          },
        ),
      );
      // pushPage completes on pop, before the covering route stops painting.
      await covering?.completed;
    } finally {
      _pushingConversation = false;
      if (!_closed && _covered) {
        setState(() => _covered = false);
        unawaited(_watch());
      }
    }
  }

  void _observeCover(PageRoute<dynamic> route) {
    final animation = route.animation;
    if (animation == null) return;
    void covered(AnimationStatus status) {
      if (!status.isCompleted || route.offstage || _covered) return;
      animation.removeStatusListener(covered);
      if (_closed || _ownerEnded || !route.isActive) return;
      final distance = _scroll.hasClients ? _scroll.position.extentBefore : 0.0;
      _returnOffset ??= distance < _followDistance ? 0 : distance;
      _covered = true;
      _error = null;
      _keep(persist: true);
      _payload = null;
      _cached = false;
      _discardView();
    }

    // Hero lays out the incoming route offstage with a completed proxy before
    // restoring the real entrance animation at the end of the frame.
    animation.addStatusListener(covered);
    if (animation.isCompleted) {
      WidgetsBinding.instance.addPostFrameCallback(
        (_) => covered(animation.status),
      );
    }
  }

  bool _openReplyLink(String link) {
    if (!widget.host.hasConnection || widget.host.isDemo) return false;
    final linked = linkedSession(link, _view.value);
    if (linked == null) return false;
    if (_closed || _ownerEnded) {
      _failure(context.tr('conversationUnavailable'));
      return true;
    }
    if (linked.id == widget.sessionId) return true;
    final child = linked.child;
    if (child != null) {
      _openChild(child);
      return true;
    }
    final session = objects(
      widget.host.snapshot['sessions'],
    ).where((session) => session['id'] == linked.id).firstOrNull;
    if (session == null) {
      _failure(context.tr('conversationUnavailable'));
    } else {
      unawaited(_openSession(session));
    }
    return true;
  }

  _StatisticsState _statisticsState() {
    final view = _view.value;
    final statistics = _snapshot['statistics'];
    if (statistics is Map<String, dynamic>) {
      return (data: statistics, statusKey: null);
    }
    if (statistics != null ||
        view['statistics_error'] != null ||
        _error != null) {
      return (data: null, statusKey: 'settingsLoadFailed');
    }
    final fault =
        view['error'] ?? (widget.host.connected ? null : widget.host.error);
    if (!widget.host.connected ||
        fault != null ||
        (view['connected'] == false && _snapshot.isNotEmpty)) {
      final code = fault is BridgeFailure ? fault.code : object(fault)['code'];
      final recovering =
          fault == null || const {'unavailable', 'busy'}.contains(code);
      return (
        data: null,
        statusKey: recovering
            ? 'conversationReconnecting'
            : 'conversationUnavailable',
      );
    }
    return (data: null, statusKey: null);
  }

  void _updateStatistics() {
    final sheet = _statisticsSheet;
    if (sheet != null && !_closed && !_ownerEnded && !_covered) {
      sheet.value = _statisticsState();
    }
  }

  void _showStatistics() {
    if (_closed || _ownerEnded || _covered || _statisticsSheet != null) return;
    final sheet = ValueNotifier(_statisticsState());
    _statisticsSheet = sheet;
    void release() {
      if (!identical(_statisticsSheet, sheet)) return;
      _statisticsRoute = null;
      _statisticsSheet = null;
      sheet.dispose();
    }

    unawaited(
      showAppSheet<void>(
        context,
        context.tr('conversationStats'),
        child: Builder(
          builder: (context) {
            final route = ModalRoute.of(context)!;
            if (_statisticsRoute == null) {
              _statisticsRoute = route;
              unawaited(route.completed.then((_) => release()));
            }
            if (_closed ||
                _ownerEnded ||
                _covered ||
                (!route.isCurrent &&
                    route.animation?.status != AnimationStatus.reverse)) {
              _dismissStatistics(route);
              return const SizedBox.shrink();
            }
            return ValueListenableBuilder(
              valueListenable: sheet,
              builder: (context, state, _) => ConversationStatistics(
                data: state.data,
                statusKey: state.statusKey,
              ),
            );
          },
        ),
      ).then((_) {
        if (_statisticsRoute == null) release();
      }),
    );
  }

  void _dismissStatistics(ModalRoute<dynamic>? route) {
    if (route == null || !route.isActive) return;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (route.isActive) route.navigator?.removeRoute(route);
    });
    WidgetsBinding.instance.ensureVisualUpdate();
  }

  void _configuration() => showAppSheet(
    context,
    context.tr('modelPicker'),
    child: ConversationConfiguration(host: widget.host, session: _session),
  );

  @override
  Widget build(BuildContext context) {
    // A transcript shorter than the viewport cannot be scrolled to load more.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _loadOlderNearTop();
    });
    return ValueListenableBuilder(
      valueListenable: _view,
      builder: (context, view, _) {
        final connected =
            widget.host.connected &&
            view['connected'] == true &&
            _error == null;
        final queued = summarize(view, _submission.guidance);
        final queue = queued.count == 0
            ? null
            : QueueChip(state: queued, onOpen: _queue);
        final delegated = _session['delegation'] != null;
        final fault = view['error'];
        final hostFault = widget.host.connected ? null : widget.host.error;
        final observedFaults = [fault, hostFault].nonNulls;
        final faults = [_error, ...observedFaults].nonNulls;
        final failureKey = faults.isEmpty
            ? null
            : faults.map(versionMismatchKey).nonNulls.firstOrNull ??
                  faults.map(connectionFailureKey).nonNulls.firstOrNull ??
                  'connectionFailureUnknown';
        // These are the observer's retryable faults, not an application retry loop.
        final reconnecting =
            !_ownerEnded &&
            _error == null &&
            observedFaults.every(
              (error) =>
                  const {'unavailable', 'busy'}.contains(object(error)['code']),
            );
        final retry =
            !_ownerEnded && !_retrying && (!_watching || _updates != null)
            ? _retry
            : null;
        final actionStyle = IconButton.styleFrom(
          fixedSize: const Size.square(40),
          minimumSize: const Size.square(40),
          padding: EdgeInsets.zero,
          tapTargetSize: MaterialTapTargetSize.shrinkWrap,
        );
        final loading =
            !_ownerEnded &&
            _snapshot.isEmpty &&
            faults.isEmpty &&
            widget.host.connected;
        final unavailable = !connected;
        final hasTranscript =
            (_page['entries'] as List? ?? const []).isNotEmpty ||
            _transcript.runs.isNotEmpty ||
            (_snapshot['drafts'] as List? ?? const []).isNotEmpty;
        // A cached transcript waiting for its live view is not a disconnect;
        // the host status line shows the reconnection.
        final showConnectionBanner =
            unavailable && hasTranscript && !(_cached && faults.isEmpty);
        // A pending watch or refresh reads as reconnecting rather than a retry.
        final recovering = !_ownerEnded && (reconnecting || retry == null);
        final hasBanner =
            _historyPending != null || _backup != null || _pendingSend != null;
        return ConversationFrame(
          title: null,
          semanticTitle: text(_session['title'], context.tr('chat')),
          actions: [
            Surface(
              key: const ValueKey('conversation-actions'),
              radius: 20,
              padding: const EdgeInsets.symmetric(horizontal: 4),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  IconButton(
                    key: const ValueKey('conversation-info'),
                    style: actionStyle,
                    icon: const AppIcon('info', size: 18),
                    tooltip: context.tr('conversationStats'),
                    onPressed: _ownerEnded ? null : _showStatistics,
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
          bodyBuilder: (context, bottomInset) => Stack(
            children: [
              LayoutBuilder(
                builder: (context, constraints) => Column(
                  children: [
                    if (hasBanner)
                      SizedBox(height: ConversationFrame.contentInset(context)),
                    if (_historyPending != null)
                      MaterialBanner(
                        content: Text(context.tr('conversationUnknown')),
                        actions: [
                          TextButton(
                            onPressed: _historyBusy ? null : _checkHistory,
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
                            onPressed: _sending || !connected
                                ? null
                                : _checkSend,
                            child: Text(context.tr('conversationCheckResult')),
                          ),
                        ],
                      ),
                    Expanded(
                      child: unavailable && !hasTranscript
                          ? Padding(
                              padding: EdgeInsets.only(
                                top: ConversationFrame.contentInset(context),
                                bottom: bottomInset,
                              ),
                              child: loading
                                  ? DeferredLoading(
                                      label: context.tr('conversationLoading'),
                                    )
                                  : DisconnectedState(
                                      reconnecting: recovering,
                                      failureKey: failureKey,
                                      onRetry: retry,
                                    ),
                            )
                          : connected && !hasTranscript
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
                          : !_settled
                          ? Padding(
                              padding: EdgeInsets.only(
                                top: ConversationFrame.contentInset(context),
                                bottom: bottomInset,
                              ),
                              child: DeferredLoading(
                                label: context.tr('conversationLoading'),
                              ),
                            )
                          : LiveTimeline(
                              key: ValueKey((widget.host.id, widget.sessionId)),
                              controller: _scroll,
                              padding: EdgeInsets.fromLTRB(
                                20,
                                hasBanner
                                    ? 0
                                    : ConversationFrame.contentInset(context),
                                20,
                                bottomInset + 8,
                              ),
                              older: [
                                if (view['older_error'] != null)
                                  Text(
                                    key: const ValueKey('older-error'),
                                    context.tr(
                                      connectionFailureKey(
                                            view['older_error'],
                                          ) ??
                                          'conversationFailed',
                                    ),
                                  ),
                                if (_hasOlder && view['loading_older'] == true)
                                  const Padding(
                                    key: ValueKey('older'),
                                    padding: EdgeInsets.symmetric(vertical: 12),
                                    child: Center(
                                      child: SizedBox.square(
                                        dimension: 18,
                                        child: CircularProgressIndicator(
                                          strokeWidth: 2,
                                        ),
                                      ),
                                    ),
                                  ),
                              ],
                              transcript: _transcript,
                              connected: view['connected'] == true,
                              session: _session,
                              host: widget.host,
                              command: _command,
                              onPlanFeedback: _sendPlanFeedback,
                              pendingPlanFeedback: _pendingPlanFeedback,
                              toolOutput: _trackTools,
                              onOpenChild: _openChild,
                              onOpenSession: _openReplyLink,
                              onTurnAction: _canUseHistory ? _turnAction : null,
                              canChangeHistory: _canChangeHistory,
                              sending: _sending || _pendingSend != null,
                              artifactActions: _artifactActions,
                              artifactChanges: _view,
                              canRestore: () => _canChangeHistory,
                              turnKey: (turn) =>
                                  _turnKeys.putIfAbsent(turn, GlobalKey.new),
                            ),
                    ),
                  ],
                ),
              ),
              if (hasTranscript)
                Positioned(
                  top: ConversationFrame.contentInset(context) - 4,
                  left: 20,
                  right: 20,
                  child: Center(
                    child: ConnectionBanner(
                      visible: showConnectionBanner,
                      reconnecting: recovering,
                      failureKey: failureKey,
                      onRetry: retry,
                    ),
                  ),
                ),
            ],
          ),
          composer: SafeArea(
            top: false,
            left: false,
            right: false,
            bottom: delegated,
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                if (!_ownerEnded)
                  ActivityStrip(
                    key: ValueKey((
                      'activity',
                      widget.host.id,
                      widget.sessionId,
                    )),
                    host: widget.host,
                    session: _session,
                    updates: _view,
                    queue: queue,
                    onOpenChild: _openChild,
                  )
                else if (queue != null)
                  ComposerChipRow(children: [queue]),
                if (!delegated)
                  MessageComposer(
                    controller: _draft,
                    attachments: _attachments.isEmpty
                        ? null
                        : DraftAttachments(
                            attachments: _attachments,
                            onRemove: (attachment) =>
                                setState(() => _attachments.remove(attachment)),
                          ),
                    attached: false,
                    attachmentsEnabled: !widget.host.isDemo,
                    busy: _active.isNotEmpty,
                    enabled:
                        connected &&
                        !_historyBusy &&
                        _historyPending == null &&
                        !_sending &&
                        _pendingSend == null &&
                        _session['archived'] != true,
                    onAttachment: (_) => _attach(),
                    onStop: _stop,
                    onSend: _send,
                  ),
              ],
            ),
          ),
        );
      },
    );
  }
}
