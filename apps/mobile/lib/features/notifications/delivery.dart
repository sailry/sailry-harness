import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:fluttertoast/fluttertoast.dart';

import '../../l10n/strings.dart';
import '../../runtime/json.dart';
import '../../runtime/session.dart';
import '../../ui/kit.dart';
import '../../ui/toast.dart';
import '../conversations/live/page.dart';
import 'presentation.dart';
import 'system.dart';

/// Presentation only: the shared Client inbox owns notices and read state.
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
  final _freshness = NoticeFreshness();
  Set<String> _registeredRoutes = const {};
  Object? _owner;
  bool _reading = false;
  bool _pending = false;
  final _toast = FToast();
  final _system = SystemAlerts();
  StreamSubscription<SessionTarget>? _taps;
  SessionTarget? _pendingTap;
  Future<bool>? _starting;

  @override
  void initState() {
    super.initState();
    widget.session.addListener(_changed);
    _taps = _system.taps.listen((target) {
      _pendingTap = target;
      _openTapped();
    });
    unawaited(_read());
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (_starting != null) return;
    _starting = _system.start(context.tr);
    if (widget.enabled) unawaited(_requestPermission());
    unawaited(
      _starting!.then((_) {
        if (mounted) unawaited(_read());
      }),
    );
  }

  @override
  void didUpdateWidget(NotificationDelivery oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!oldWidget.enabled && widget.enabled) unawaited(_requestPermission());
  }

  Future<void> _requestPermission() async {
    try {
      if (await _starting == true && mounted && widget.enabled) {
        await widget.session.background.requestNotificationPermission();
      }
    } catch (_) {
      // System permission failures do not affect the in-app inbox.
    }
  }

  /// Opens the session a system notification named, once its host has
  /// connected and lists it.
  void _openTapped() {
    final target = _pendingTap;
    if (target == null || !mounted) return;
    final host = widget.session.host(target.host);
    final session = host == null
        ? null
        : objects(
            host.snapshot['sessions'],
          ).where((value) => value['id'] == target.session).firstOrNull;
    if (host == null || !host.connected || session == null) return;
    _pendingTap = null;
    pushPage(
      context,
      LiveConversationPage(
        host: host,
        sessionId: target.session,
        initialSession: session,
      ),
    );
  }

  void _changed() {
    _openTapped();
    unawaited(_read());
  }

  Future<void> _read() async {
    if (_reading) {
      _pending = true;
      return;
    }
    final owner = widget.session.controller;
    if (owner == null) return;
    if (_owner != owner) {
      _owner = owner;
      _freshness.clear();
      _registeredRoutes = const {};
    }
    _reading = true;
    try {
      do {
        _pending = false;
        final hosts = widget.session.hosts;
        final registeredRoutes = {for (final host in hosts) host.id};
        final refreshedRoutes = {
          for (final host in hosts)
            if (host.connected && host.snapshot['node'] != null) host.id,
        };
        final inbox = object(jsonDecode(await owner.notifications()));
        if (!mounted || widget.session.controller != owner) return;
        final notices = objects(inbox['notices']);
        final fresh = _freshness.observe(
          notices,
          refreshedRoutes,
          registeredRoutes,
        );
        final foreground = WidgetsBinding.instance.lifecycleState;
        final front =
            foreground == null || foreground == AppLifecycleState.resumed;
        // In the background the app posts system notifications instead.
        await _system.sync(
          notices,
          fresh: widget.enabled && !front ? fresh : const {},
          refreshedRoutes: refreshedRoutes,
          forgottenRoutes: _registeredRoutes.difference(registeredRoutes),
          translate: context.tr,
        );
        if (!mounted || widget.session.controller != owner) return;
        _registeredRoutes = registeredRoutes;
        if (widget.enabled && front) {
          for (final notice in notices.reversed.where(
            (notice) =>
                completedSessionNotice(notice) &&
                fresh.contains(jsonEncode(notice['id'])),
          )) {
            showToast(
              context,
              '${noticeTitle(notice, translate: context.tr)} · ${context.tr('completed')}',
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
    unawaited(_taps?.cancel());
    unawaited(_system.close());
    _toast.removeQueuedCustomToasts();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => widget.child;
}
