import 'dart:async';

import 'package:flutter/foundation.dart';

/// Local paint timing; cursor mode and position still come from the Node.
class CursorBlink extends ValueNotifier<bool> {
  CursorBlink() : super(true);

  Timer? _timer;
  bool _enabled = false;

  void update({required bool enabled, bool restart = false}) {
    if (_enabled == enabled && !restart) return;
    _enabled = enabled;
    _timer?.cancel();
    _timer = null;
    value = true;
    if (enabled) {
      _timer = Timer.periodic(const Duration(milliseconds: 530), (_) {
        value = !value;
      });
    }
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }
}
