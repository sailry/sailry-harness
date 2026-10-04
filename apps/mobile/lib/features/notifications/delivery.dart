import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:fluttertoast/fluttertoast.dart';

import '../../l10n/strings.dart';
import '../../runtime/json.dart';
import '../../runtime/session.dart';
import '../../ui/toast.dart';
import 'presentation.dart';

/// Presentation only: the shared Rust inbox owns notices and read state.
class NotificationDelivery extends StatefulWidget {
  const NotificationDelivery({
    super.key,
    required this.session,
    required this.enabled,
    required this.child,
  });
  final AppSession session;
  final bool enabled;
  final Widget child;
  @override
  State<NotificationDelivery> createState() => _NotificationDeliveryState();
}

class _NotificationDeliveryState extends State<NotificationDelivery> {
  Set<String>? _shown;
  Object? _owner;
  bool _reading = false;
  bool _pending = false;
  final _toast = FToast();

  @override
  void initState() {
    super.initState();
    widget.session.addListener(_changed);
    unawaited(_read());
  }

  void _changed() => unawaited(_read());

  Future<void> _read() async {
    if (_reading) {
      _pending = true;
      return;
    }
    final owner = widget.session.controller;
    if (owner == null) return;
    if (_owner != owner) {
      _owner = owner;
      _shown = null;
    }
    _reading = true;
    try {
      do {
        _pending = false;
        final inbox = object(jsonDecode(await owner.notifications()));
        if (!mounted || widget.session.controller != owner) return;
        final notices = objects(inbox['notices']);
        final previous = _shown;
        // Keep only presentation identities in the bounded Rust snapshot. Turning
        // alerts off or going to the background never discards inbox history.
        _shown = notices.map((notice) => jsonEncode(notice['id'])).toSet();
        final foreground = WidgetsBinding.instance.lifecycleState;
        if (previous != null &&
            widget.enabled &&
            (foreground == null || foreground == AppLifecycleState.resumed)) {
          for (final notice in notices.reversed.where(
            (notice) =>
                notice['read'] != true &&
                notice['kind'] == 'Completed' &&
                object(object(notice['id'])['target']).containsKey('Session') &&
                !previous.contains(jsonEncode(notice['id'])),
          )) {
            showToast(
              context,
              '${noticeTitle(notice)} · ${tr('completed')}',
              icon: 'check',
              key: const ValueKey('completion-toast'),
            );
          }
        }
      } while (_pending && mounted);
    } catch (_) {
      // A transient toast must not replace the current task with an error banner.
    } finally {
      _reading = false;
    }
  }

  @override
  void dispose() {
    widget.session.removeListener(_changed);
    _toast.removeQueuedCustomToasts();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => widget.child;
}
