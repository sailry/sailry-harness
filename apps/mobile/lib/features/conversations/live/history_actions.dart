part of 'page.dart';

extension _HistoryActions on _LiveConversationPageState {
  bool get _canUseHistory =>
      widget.host.connected &&
      _view.value['connected'] == true &&
      _error == null &&
      !_historyBusy &&
      _historyPending == null &&
      !_sending &&
      _pendingSend == null;

  bool get _canChangeHistory =>
      _canUseHistory &&
      _session['delegation'] == null &&
      _session['archived'] != true &&
      (object(_page['queue'])['items'] as List? ?? const []).isEmpty &&
      _transcript.latestActive == null;

  Future<bool> _historyCommand(String kind, Map<String, dynamic> data) =>
      _finishHistory(() => _history.submit(kind, data));

  Future<bool> _checkHistory() => _finishHistory(_history.check);

  Future<bool> _finishHistory(
    Future<HistoryCompletion?> Function() operation,
  ) async {
    if (_closed || _ownerEnded) return false;
    try {
      final result = await operation();
      if (result == null) return false;
      final (:kind, :output) = result;
      if (!mounted ||
          _ownerEnded ||
          _route?.isActive == false ||
          _route?.animation?.status == AnimationStatus.reverse ||
          kind == 'fork_conversation' && _route?.isCurrent == false ||
          !_history.consume(result)) {
        return true;
      }
      if (kind == 'rewind_conversation' || kind == 'replace_turn') {
        _backup = historyBackup(kind, output);
      }
      _rebuild();
      if (kind == 'fork_conversation') {
        unawaited(_openSession(object(output['data'])));
      }
      return true;
    } catch (failure) {
      _failure(failure);
      return false;
    }
  }

  Future<void> _turnAction(String action, String turn) async {
    if (!_canUseHistory || action != 'fork' && !_canChangeHistory) return;
    final page = _page;
    final run = _transcript.runOf(turn);
    if (run == null || activeRun(run)) return;
    if (action == 'edit' && !editableTurn(run)) return;
    final head = _transcript.runs.last['turn'];
    final revision = page['revision'];
    final sessionRevision = _session['revision'];
    final input = sentInput(page, turn);
    if (['edit', 'retry'].contains(action) &&
        !objects(
          page['entries'],
        ).any((entry) => entry['turn'] == turn && entry['author'] == 'user')) {
      return;
    }
    switch (action) {
      case 'fork':
        await _historyCommand('fork_conversation', {
          'session': widget.sessionId,
          'through': turn,
          'expected_revision': sessionRevision,
        });
      case 'retry':
        if (!['failed', 'interrupted'].contains(run['status'])) return;
        await _historyCommand('submit_turn', {
          'session': widget.sessionId,
          'expected_revision': sessionRevision,
          'message': input,
        });
      case 'edit':
        await pushPage(
          context,
          MessageEditor(
            initial: _edits[turn] ?? text(input['text']),
            onChanged: (value) => _edits[turn] = value,
            isPending: () => _historyPending == 'replace_turn',
            onCheck: () async {
              if (_historyPending != 'replace_turn') return false;
              final accepted = await _checkHistory();
              if (accepted) _edits.remove(turn);
              return accepted;
            },
            onSubmit: (value) async {
              if (!_canChangeHistory ||
                  _transcript.runs.lastOrNull?['turn'] != head ||
                  _page['revision'] != revision ||
                  _session['revision'] != sessionRevision) {
                _failure(context.tr('conversationConflict'));
                return false;
              }
              final success = await _historyCommand('replace_turn', {
                'session': widget.sessionId,
                'turn': turn,
                'expected_head': head,
                'expected_history_revision': revision,
                'expected_revision': sessionRevision,
                'message': {...input, 'text': value},
              });
              if (success) _edits.remove(turn);
              return success;
            },
          ),
        );
      case 'rewind':
        if (turn == head) return;
        final confirmed = await showAppDialog<bool>(
          context: context,
          builder: (context) => AlertDialog(
            title: Text(context.tr('messageRewind')),
            content: Text(context.tr('messageRewindConfirm')),
            actions: [
              TextButton(
                onPressed: () => Navigator.pop(context, false),
                child: Text(context.tr('cancel')),
              ),
              FilledButton(
                onPressed: () => Navigator.pop(context, true),
                child: Text(context.tr('confirm')),
              ),
            ],
          ),
        );
        if (!mounted || confirmed != true) return;
        if (!_canChangeHistory ||
            _page['revision'] != revision ||
            _transcript.runs.lastOrNull?['turn'] != head) {
          _failure(context.tr('conversationConflict'));
          return;
        }
        await _historyCommand('rewind_conversation', {
          'session': widget.sessionId,
          'through': turn,
          'expected_head': head,
          'expected_revision': revision,
        });
    }
  }

  Future<void> _searchMessages() async {
    final result = await pushPage<Map<String, dynamic>>(
      context,
      ConversationSearch(host: widget.host, session: widget.sessionId),
    );
    if (!mounted || result == null) return;
    if (result['revision'] != _page['revision']) {
      _failure(context.tr('messageSearchStale'));
      return;
    }
    _reveal = (turn: text(result['turn']), revision: result['revision'] as int);
    if (_transcript.containsTurn(_reveal!.turn)) {
      _revealMessage();
    } else {
      try {
        await _updates?.loadThrough(
          sequence: BigInt.parse('${result['turn_sequence']}'),
        );
      } catch (failure) {
        _reveal = null;
        _failure(failure);
      }
    }
  }

  void _revealMessage() {
    final target = _reveal;
    if (target == null) return;
    if (_page['revision'] != target.revision) {
      _reveal = null;
      _failure(context.tr('messageSearchStale'));
      return;
    }
    if (!_transcript.containsTurn(target.turn)) {
      if (_view.value['older_error'] != null) {
        _reveal = null;
        _failure(context.tr('conversationFailed'));
      } else if (_page['next_before'] == null &&
          _view.value['loading_older'] != true) {
        _reveal = null;
        _failure(context.tr('messageSearchMissing'));
      }
      return;
    }
    if (_seeking) return;
    _seeking = true;
    WidgetsBinding.instance.addPostFrameCallback((_) => _seek());
  }

  /// Turns are built lazily, so an unbuilt target is approached one viewport
  /// per frame until it is laid out, then revealed precisely. One step runs
  /// per frame so no turn between the two positions is skipped unbuilt.
  void _seek() {
    _seeking = false;
    final target = _reveal;
    if (!mounted || target == null) return;
    final owner = turnOwner(_view.value, target.turn);
    final targetContext = _turnKeys[owner]?.currentContext;
    if (targetContext != null) {
      _reveal = null;
      Scrollable.ensureVisible(
        targetContext,
        alignment: .2,
        duration: const Duration(milliseconds: 250),
      );
      return;
    }
    if (!_scroll.hasClients) return;
    final goal = _transcript.positionOf(owner);
    if (goal == null) return;
    int? shown;
    for (final item in _turnKeys.entries) {
      if (item.value.currentContext == null) continue;
      final position = _transcript.positionOf(item.key);
      if (position != null && (shown == null || position < shown)) {
        shown = position;
      }
    }
    final position = _scroll.position;
    // Older turns sit at larger offsets in the reversed transcript.
    final step = shown == null || goal < shown
        ? position.viewportDimension
        : -position.viewportDimension;
    final next = (position.pixels + step).clamp(
      position.minScrollExtent,
      position.maxScrollExtent,
    );
    if (next == position.pixels) return;
    position.jumpTo(next);
    _seeking = true;
    WidgetsBinding.instance.addPostFrameCallback((_) => _seek());
  }
}
