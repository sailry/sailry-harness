import 'package:flutter/foundation.dart';

import '../../../runtime/session.dart';

typedef HistoryCompletion = ({String kind, Map<String, dynamic> output});

class HistoryWrite extends ChangeNotifier {
  HistoryWrite._(this.host, this.session);

  static final _writes = Expando<Map<String, HistoryWrite>>();

  static HistoryWrite retain(HostConnection host, String session) {
    final writes = _writes[host] ??= {};
    final write = writes.putIfAbsent(
      session,
      () => HistoryWrite._(host, session),
    );
    write._views++;
    return write;
  }

  final HostConnection host;
  final String session;
  ({String kind, String request})? _pending;
  HistoryCompletion? _completed;
  bool _busy = false;
  int _views = 0;

  bool get busy => _busy;
  String? get pendingKind => _pending?.kind ?? _completed?.kind;

  Future<HistoryCompletion?> submit(
    String kind,
    Map<String, dynamic> data,
  ) async {
    if (busy || pendingKind != null) return null;
    return _perform(kind, () => host.command(kind, data));
  }

  Future<HistoryCompletion?> check() async {
    if (busy) return null;
    if (_completed case final completed?) return completed;
    final pending = _pending;
    if (pending == null) return null;
    return _perform(pending.kind, () => host.check(pending.request));
  }

  Future<HistoryCompletion> _perform(
    String kind,
    Future<Map<String, dynamic>> Function() operation,
  ) async {
    _busy = true;
    notifyListeners();
    try {
      final result = (kind: kind, output: await operation());
      _pending = null;
      _completed = result;
      return result;
    } on CommandFailure catch (error) {
      if (error.retained) _pending = (kind: kind, request: error.request!);
      if (error.rejected) _pending = null;
      rethrow;
    } finally {
      _busy = false;
      notifyListeners();
      _releaseIfSettled();
    }
  }

  bool consume(HistoryCompletion result) {
    if (_completed != result) return false;
    _completed = null;
    notifyListeners();
    _releaseIfSettled();
    return true;
  }

  void release() {
    _views--;
    _releaseIfSettled();
  }

  void _releaseIfSettled() {
    if (_views != 0 || busy || pendingKind != null) return;
    final writes = _writes[host];
    if (writes?[session] != this) return;
    writes!.remove(session);
    dispose();
  }
}
