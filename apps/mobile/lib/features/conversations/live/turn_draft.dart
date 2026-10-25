import 'package:flutter/widgets.dart';

import '../../../runtime/session.dart';
import 'attachments.dart';

typedef PendingMessage = ({
  String request,
  String text,
  List<PickedAttachment> attachments,
  bool composer,
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
  bool _sending = false;
  bool _disposed = false;

  PendingMessage? get pending => _pending;
  bool get sending => _sending;

  Future<void> send(int revision, {String? message}) async {
    final composer = message == null;
    final text = message ?? input.text;
    final files = composer ? List.of(attachments) : <PickedAttachment>[];
    if (sending || pending != null || !host.connected) return;
    if (composer && text.trim().isEmpty && files.isEmpty) return;
    _sending = true;
    notifyListeners();
    try {
      await host.command('submit_turn', {
        'session': session,
        'expected_revision': revision,
        'message': {
          'text': text,
          'attachments': files.map((item) => item.attachment['id']).toList(),
        },
      });
      if (!_disposed && composer) _clear(text, files);
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
      await host.execute(captured.request);
      if (_disposed) return false;
      if (captured.composer) _clear(captured.text, captured.attachments);
      _pending = null;
      return true;
    } catch (error) {
      if (!_disposed && error is CommandFailure && error.rejected) {
        _pending = null;
      }
      rethrow;
    } finally {
      if (!_disposed) {
        _sending = false;
        notifyListeners();
      }
    }
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
