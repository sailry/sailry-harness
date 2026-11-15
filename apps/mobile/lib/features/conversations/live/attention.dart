import '../../../runtime/json.dart';
import '../../../runtime/session.dart';

final _attentionStates = Expando<Map<String, _AttentionState>>();

class _AttentionState {
  int? attempted;
  String? pending;
  bool running = false;
  ConversationAttention? next;
}

/// Background read acknowledgements never report conversation command failures.
class ConversationAttention {
  ConversationAttention({
    required this.host,
    required this.sessionId,
    required this.session,
    required this.active,
  }) : _state = (_attentionStates[host] ??= {}).putIfAbsent(
         sessionId,
         _AttentionState.new,
       );

  final HostConnection host;
  final String sessionId;
  final Map<String, dynamic> Function() session;
  final bool Function() active;
  // Keep request identity across navigation for this host's lifetime.
  final _AttentionState _state;
  bool _closed = false;

  Future<void> observe() async {
    if (_closed || !active() || !host.connected) return;
    if (_state.running) {
      _state.next = this;
      return;
    }
    _state.running = true;
    final pending = _state.pending;
    try {
      if (pending != null) {
        await host.check(pending);
        _state.pending = null;
        _state.next ??= this;
      } else {
        final attention = object(object(session()['activity'])['attention']);
        final revision = attention['revision'];
        if (attention['unread'] != true) {
          _state.attempted = null;
          return;
        }
        if (revision is! int || revision == _state.attempted) {
          return;
        }
        _state.attempted = revision;
        await host.command('set_session_read', {
          'session': sessionId,
          'expected_revision': revision,
          'read': true,
        });
      }
    } on CommandFailure catch (failure) {
      if (pending != null) {
        if (failure.rejected) {
          _state.pending = null;
          _state.next ??= this;
        }
      } else if (failure.retained) {
        _state.pending = failure.request;
      }
    } catch (_) {
      // Read markers are best effort; a retained request stays unresolved.
    } finally {
      _state.running = false;
      final next = _state.next;
      _state.next = null;
      if (next != null) await next.observe();
    }
  }

  void dispose() {
    _closed = true;
    if (identical(_state.next, this)) _state.next = null;
  }
}
