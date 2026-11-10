import 'package:flutter/foundation.dart';

/// Forms reuse the captured conversation's admission and pending request owner.
class ArtifactActions extends ChangeNotifier {
  ArtifactActions({
    required this.submit,
    required this.check,
    required this.writable,
    required this.pending,
  });

  final Future<void> Function(String message) submit;
  final Future<bool> Function() check;
  final bool Function() writable;
  final bool Function() pending;

  (bool, bool)? _last;

  void changed() {
    final state = (writable(), pending());
    if (state == _last) return;
    _last = state;
    notifyListeners();
  }
}
