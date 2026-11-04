import 'package:flutter/widgets.dart';

import '../../../runtime/session.dart';
import '../../../runtime/json.dart';
import 'attachments.dart';

typedef PendingMessage = ({
  String request,
  String text,
  List<PickedAttachment> attachments,
  bool composer,
});

typedef GuidancePreview = ({
  String? turn,
  String text,
  List<PickedAttachment> attachments,
});

/// Owns the composer's captured input while a durable command is unresolved.
class TurnDraft extends ChangeNotifier {
  TurnDraft({
    required this.host,
    required this.session,
    String text = '',
    List<PickedAttachment> attachments = const [],
  }) : input = TextEditingController(text: text),
       attachments = List.of(attachments);

  final HostConnection host;
  final String session;
  final TextEditingController input;
  final List<PickedAttachment> attachments;
  PendingMessage? _pending;
  GuidancePreview? _guidance;
  bool _sending = false;
  bool _disposed = false;

  PendingMessage? get pending => _pending;
  bool get sending => _sending;
  GuidancePreview? get guidance => _guidance;

  Future<void> send(
    int revision, {
    String? message,
    ({String turn, int revision})? recipient,
    bool immediate = false,
    bool queue = false,
  }) async {
    final composer = message == null;
    final text = message ?? input.text;
    final files = composer ? List.of(attachments) : <PickedAttachment>[];
    if (sending || pending != null || !host.connected) return;
    if (composer && text.trim().isEmpty && files.isEmpty) return;
    _sending = true;
    _guidance = recipient == null || queue
        ? null
        : (turn: null, text: text, attachments: files);
    notifyListeners();
    try {
      final output = await host.command(
        queue
            ? 'queue_turn'
            : recipient == null
            ? 'submit_turn'
            : 'guide_turn',
        {
          if (recipient == null || queue) 'session': session,
          if (recipient != null && !queue) 'recipient': recipient.turn,
          'expected_revision': !queue && recipient != null
              ? recipient.revision
              : revision,
          if (recipient != null && !queue) 'immediate': immediate,
          'message': {
            'text': text,
            'attachments': files.map((item) => item.attachment['id']).toList(),
          },
        },
      );
      if (!_disposed) {
        _admitted(output);
        if (composer) _clear(text, files);
      }
    } catch (error) {
      if (!_disposed &&
          error is CommandFailure &&
          error.code == 'outcome_unknown' &&
          error.request != null) {
        _pending = (
          request: error.request!,
          text: text,
          attachments: files,
          composer: composer,
        );
      } else if (!_disposed) {
        _guidance = null;
      }
      rethrow;
    } finally {
      if (!_disposed) {
        _sending = false;
        notifyListeners();
      }
    }
  }

  Future<bool> check() async {
    final captured = pending;
    if (captured == null || sending) return false;
    _sending = true;
    notifyListeners();
    try {
      // Reconciliation queries the original request; it never resubmits input.
      final output = await host.execute(captured.request);
      if (_disposed) return false;
      _admitted(output);
      if (captured.composer) _clear(captured.text, captured.attachments);
      _pending = null;
      return true;
    } catch (error) {
      if (!_disposed && error is CommandFailure && error.rejected) {
        _pending = null;
        _guidance = null;
      }
      rethrow;
    } finally {
      if (!_disposed) {
        _sending = false;
        notifyListeners();
      }
    }
  }

  /// Retires the optimistic preview once the projected [guidance] shows its
  /// admitted turn, so a later rewind or replacement cannot revive it.
  ///
  /// Unadmitted and uncertain submissions stay visible. This does not notify:
  /// the observed projection already hides the preview.
  void observe(Map<String, dynamic> guidance) {
    final turn = _guidance?.turn;
    if (turn != null && guidance.containsKey(turn)) _guidance = null;
  }

  void _admitted(Map<String, dynamic> output) {
    final captured = _guidance;
    if (captured == null) return;
    _guidance = (
      turn: object(output['data'])['id'] as String?,
      text: captured.text,
      attachments: captured.attachments,
    );
  }

  void _clear(String text, List<PickedAttachment> files) {
    // Preserve typing and files added while admission was pending.
    if (input.text == text) input.clear();
    attachments.removeWhere(files.contains);
  }

  @override
  void dispose() {
    _disposed = true;
    input.dispose();
    super.dispose();
  }
}
