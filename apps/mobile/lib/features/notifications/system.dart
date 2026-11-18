import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_local_notifications/flutter_local_notifications.dart';

import '../../l10n/strings.dart';
import '../../runtime/json.dart';
import 'presentation.dart';

/// Where a tapped system notification leads: a session on a specific host route.
typedef SessionTarget = ({String host, String session});

/// Android system notifications for session notices that arrive while the
/// app is in the background. The foreground service keeps the process and
/// its host links alive; this only posts what the shared Client inbox
/// already holds, and withdraws a notice once it is read or gone.
class SystemAlerts {
  SystemAlerts({bool? supported, FlutterLocalNotificationsPlugin? plugin})
    : supported = supported ?? Platform.isAndroid,
      _plugin = plugin ?? FlutterLocalNotificationsPlugin();

  final bool supported;
  final FlutterLocalNotificationsPlugin _plugin;
  final _taps = StreamController<SessionTarget>.broadcast();
  final _posted = <String, ({int id, String route})>{};
  // Android uses this ID to distinguish the tap intents as well as the alert.
  var _nextId = 1 << 29;
  Future<bool>? _ready;
  String _channelName = '';

  /// Taps on a posted notification, including the one that launched the app.
  Stream<SessionTarget> get taps => _taps.stream;

  static const _channel = 'session_alerts';
  static const _tagPrefix = 'sailry.session.alert:';

  /// The kinds worth interrupting the user for.
  static const kinds = {'Completed', 'Failed', 'Approval', 'Input'};

  Future<bool> _initialize() => _ready ??= () async {
    try {
      final ready = await _plugin.initialize(
        settings: const InitializationSettings(
          android: AndroidInitializationSettings('@mipmap/ic_launcher'),
        ),
        onDidReceiveNotificationResponse: (response) => _tap(response.payload),
      );
      if (ready != true) return false;
    } catch (_) {
      return false;
    }
    try {
      for (final alert in await _plugin.getActiveNotifications()) {
        final id = alert.id;
        final tag = alert.tag;
        if (id == null || tag == null || !tag.startsWith(_tagPrefix)) continue;
        final key = tag.substring(_tagPrefix.length);
        final route = noticeRoute(object(jsonDecode(key)));
        if (route == null) continue;
        _posted[key] = (id: id, route: route);
        if (id >= _nextId) _nextId = id + 1;
      }
    } catch (_) {
      // A platform without active-notification access can still post alerts.
    }
    try {
      final launch = await _plugin.getNotificationAppLaunchDetails();
      if (launch?.didNotificationLaunchApp == true) {
        _tap(launch!.notificationResponse?.payload);
      }
    } catch (_) {}
    return true;
  }();

  /// Prepares the channel and taps; call once the app is localized.
  Future<bool> start(Translator translate) async {
    if (!supported) return false;
    _channelName = translate('sessionAlerts');
    return _initialize();
  }

  void _tap(String? payload) {
    if (payload == null || _taps.isClosed) return;
    try {
      final value = object(jsonDecode(payload));
      final host = text(value['host']), session = text(value['session']);
      if (host.isNotEmpty && session.isNotEmpty) {
        _taps.add((host: host, session: session));
      }
    } catch (_) {}
  }

  /// Posts [notices] not yet shown and withdraws those no longer unread.
  /// In the foreground [fresh] is empty and withdrawal still applies.
  Future<void> sync(
    List<Map<String, dynamic>> notices, {
    required Set<String> fresh,
    required Set<String> refreshedRoutes,
    Set<String> forgottenRoutes = const {},
    required Translator translate,
  }) async {
    if (!supported || await _ready != true) return;
    final current = {
      for (final notice in notices) jsonEncode(notice['id']): notice,
    };
    final unread = <String, Map<String, dynamic>>{
      for (final MapEntry(key: key, value: notice) in current.entries)
        if (notice['read'] != true &&
            kinds.contains(notice['kind']) &&
            noticeSession(notice) != null)
          key: notice,
    };
    for (final MapEntry(key: key, value: alert) in _posted.entries.toList()) {
      // On startup an empty inbox does not yet say what the host holds.
      if (forgottenRoutes.contains(alert.route) ||
          !unread.containsKey(key) &&
              (current.containsKey(key) ||
                  refreshedRoutes.contains(alert.route))) {
        await _plugin.cancel(id: alert.id, tag: '$_tagPrefix$key');
        _posted.remove(key);
      }
    }
    for (final MapEntry(key: key, value: notice) in unread.entries) {
      if (!fresh.contains(key) || _posted.containsKey(key)) continue;
      final route = noticeRoute(object(notice['id']));
      if (route == null || forgottenRoutes.contains(route)) continue;
      final id = _nextId++;
      await _plugin.show(
        id: id,
        title: noticeTitle(notice, translate: translate),
        body: translate(switch (notice['kind']) {
          'Failed' => 'conversationFailedStatus',
          'Approval' => 'approval',
          'Input' => 'questionPending',
          _ => 'completed',
        }),
        notificationDetails: NotificationDetails(
          android: AndroidNotificationDetails(
            _channel,
            _channelName,
            tag: '$_tagPrefix$key',
            importance: Importance.defaultImportance,
            priority: Priority.defaultPriority,
            autoCancel: true,
          ),
        ),
        payload: jsonEncode({'host': route, 'session': noticeSession(notice)}),
      );
      _posted[key] = (id: id, route: route);
    }
  }

  Future<void> close() async {
    await _taps.close();
  }
}
