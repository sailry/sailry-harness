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
      objects(object(_page['queue'])['items']).isEmpty &&
      !objects(_page['runs']).any(activeRun);

  Future<bool> _historyCommand(String kind, Map<String, dynamic> data) async {
    if (_historyBusy) return false;
    final pending = _historyPending;
    if (pending != null && pending.kind != kind) return false;
    _historyBusy = true;
    _rebuild();
    try {
      final output = pending == null
          ? await _command(kind, data)
          : await widget.host.execute(pending.request);
      if (!mounted) return true;
      _historyPending = null;
      if (kind == 'rewind_conversation') {
        _backup = object(object(output['data'])['backup']);
      }
      if (kind == 'replace_turn') {
        _backup = object(object(object(output['data'])['history'])['backup']);
      }
      _rebuild();
      if (kind == 'fork_conversation') _openSession(object(output['data']));
      return true;
    } on CommandFailure catch (failure) {
      if (mounted) {
        _historyPending =
            failure.code == 'outcome_unknown' && failure.request != null
            ? (kind: kind, request: failure.request!)
            : null;
        _rebuild();
        _failure(failure);
      }
      return false;
    } catch (failure) {
      _failure(failure);
      return false;
    } finally {
      if (mounted) {
        _historyBusy = false;
        _rebuild();
      }
    }
  }

  void _openSession(Map<String, dynamic> session) {
    if (session['id'] is! String) return;
    pushPage(
      context,
      LiveConversationPage(
        host: widget.host,
        sessionId: session['id'],
        initialSession: session,
      ),
    );
  }

  Future<void> _turnAction(String action, String turn) async {
    if (!_canUseHistory || action != 'fork' && !_canChangeHistory) return;
    final page = _page;
    final run = objects(
      page['runs'],
    ).where((run) => run['turn'] == turn).firstOrNull;
    if (run == null || activeRun(run)) return;
    final head = objects(page['runs']).last['turn'];
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
            isPending: () => _historyPending?.kind == 'replace_turn',
            onSubmit: (value) async {
              if (_historyPending?.kind == 'replace_turn') {
                final accepted = await _historyCommand('replace_turn', {});
                if (accepted) _edits.remove(turn);
                return accepted;
              }
              if (!_canChangeHistory ||
                  objects(_page['runs']).lastOrNull?['turn'] != head ||
                  _page['revision'] != revision ||
                  _session['revision'] != sessionRevision) {
                _failure(tr('conversationConflict'));
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
            title: Text(tr('messageRewind')),
            content: Text(tr('messageRewindConfirm')),
            actions: [
              TextButton(
                onPressed: () => Navigator.pop(context, false),
                child: Text(tr('cancel')),
              ),
              FilledButton(
                onPressed: () => Navigator.pop(context, true),
                child: Text(tr('confirm')),
              ),
            ],
          ),
        );
        if (!mounted || confirmed != true) return;
        if (!_canChangeHistory ||
            _page['revision'] != revision ||
            objects(_page['runs']).lastOrNull?['turn'] != head) {
          _failure(tr('conversationConflict'));
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
      _failure(tr('messageSearchStale'));
      return;
    }
    _reveal = (turn: text(result['turn']), revision: result['revision'] as int);
    if (objects(_page['runs']).any((run) => run['turn'] == _reveal!.turn)) {
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
      _failure(tr('messageSearchStale'));
      return;
    }
    if (!objects(_page['runs']).any((run) => run['turn'] == target.turn)) {
      if (_view.value['older_error'] != null) {
        _reveal = null;
        _failure(tr('conversationFailed'));
      } else if (_page['next_before'] == null &&
          _view.value['loading_older'] != true) {
        _reveal = null;
        _failure(tr('messageSearchMissing'));
      }
      return;
    }
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted || _reveal != target) return;
      final targetContext = _turnKeys[target.turn]?.currentContext;
      if (targetContext != null) {
        _reveal = null;
        Scrollable.ensureVisible(
          targetContext,
          alignment: .2,
          duration: const Duration(milliseconds: 250),
        );
      }
    });
  }
}
