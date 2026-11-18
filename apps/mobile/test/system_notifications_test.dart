import 'dart:convert';

import 'package:flutter_local_notifications/flutter_local_notifications.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/notifications/presentation.dart';
import 'package:sailry_mobile/features/notifications/system.dart';

class _NotificationPlugin extends Fake
    implements FlutterLocalNotificationsPlugin {
  final shown = <({int id, String tag, String payload})>[];
  final cancelled = <({int id, String? tag})>[];
  List<ActiveNotification> active = [];
  DidReceiveNotificationResponseCallback? onTap;

  @override
  Future<bool?> initialize({
    required InitializationSettings settings,
    DidReceiveNotificationResponseCallback? onDidReceiveNotificationResponse,
    DidReceiveBackgroundNotificationResponseCallback?
    onDidReceiveBackgroundNotificationResponse,
  }) async {
    onTap = onDidReceiveNotificationResponse;
    return true;
  }

  @override
  Future<NotificationAppLaunchDetails?>
  getNotificationAppLaunchDetails() async => null;

  @override
  Future<List<ActiveNotification>> getActiveNotifications() async => active;

  @override
  Future<void> show({
    required int id,
    String? title,
    String? body,
    NotificationDetails? notificationDetails,
    String? payload,
  }) async {
    shown.add((
      id: id,
      tag: notificationDetails!.android!.tag!,
      payload: payload!,
    ));
  }

  @override
  Future<void> cancel({required int id, String? tag}) async {
    cancelled.add((id: id, tag: tag));
  }
}

Map<String, dynamic> _notice(int entry, {int cursor = 1, bool read = false}) =>
    {
      'id': {
        'node': List.filled(32, 9),
        'entry': List.filled(32, entry),
        'cursor': cursor,
        'target': {'Session': 'session'},
      },
      'session': 'session',
      'kind': 'Completed',
      'read': read,
    };

String _route(String entry) => '${entry * 32}/${'09' * 32}';

void main() {
  test(
    'each host route primes its inbox after its first confirmed snapshot',
    () {
      final freshness = NoticeFreshness();
      final first = _route('01'), second = _route('02');
      final routes = {first, second};
      final history = [_notice(1), _notice(2)];
      expect(freshness.observe([], {}, routes), isEmpty);
      expect(freshness.observe(history, {first}, routes), isEmpty);
      final nextFirst = _notice(1, cursor: 2);
      expect(freshness.observe([...history, nextFirst], routes, routes), {
        jsonEncode(nextFirst['id']),
      });
      final nextSecond = _notice(2, cursor: 2);
      expect(
        freshness.observe([...history, nextFirst, nextSecond], routes, routes),
        {jsonEncode(nextSecond['id'])},
      );
    },
  );

  test('durable completions identify the session for foreground alerts', () {
    final notice = _notice(1);
    (notice['id'] as Map)['target'] = {'Notification': 'receipt'};
    expect(completedSessionNotice(notice), isTrue);
    notice['read'] = true;
    expect(completedSessionNotice(notice), isFalse);
  });

  test(
    'restores and withdraws alerts by entry and target after restart',
    () async {
      final plugin = _NotificationPlugin();
      final alerts = SystemAlerts(supported: true, plugin: plugin);
      String translate(String key) => key;
      expect(await alerts.start(translate), isTrue);
      final notices = [_notice(1), _notice(2), _notice(3)];
      await alerts.sync(
        notices,
        fresh: {for (final notice in notices) jsonEncode(notice['id'])},
        refreshedRoutes: {},
        translate: translate,
      );
      final [first, second, third] = plugin.shown;
      expect(first.id, isNot(second.id));
      final tap = alerts.taps.first;
      plugin.onTap!(
        NotificationResponse(
          notificationResponseType:
              NotificationResponseType.selectedNotification,
          payload: second.payload,
        ),
      );
      expect(await tap, (host: _route('02'), session: 'session'));
      await alerts.close();

      plugin.active = [
        for (final alert in plugin.shown)
          ActiveNotification(id: alert.id, tag: alert.tag),
        const ActiveNotification(id: 1000, channelId: 'host_connection'),
      ];
      final restarted = SystemAlerts(supported: true, plugin: plugin);
      expect(await restarted.start(translate), isTrue);
      await restarted.sync(
        [],
        fresh: {},
        refreshedRoutes: {},
        translate: translate,
      );
      expect(plugin.cancelled, isEmpty);
      await restarted.sync(
        [],
        fresh: {},
        refreshedRoutes: {_route('01')},
        translate: translate,
      );
      expect(plugin.cancelled, [(id: first.id, tag: first.tag)]);
      await restarted.sync(
        [_notice(2, read: true)],
        fresh: {},
        refreshedRoutes: {},
        translate: translate,
      );
      expect(plugin.cancelled, [
        (id: first.id, tag: first.tag),
        (id: second.id, tag: second.tag),
      ]);
      await restarted.sync(
        [_notice(3)],
        fresh: {},
        refreshedRoutes: {},
        forgottenRoutes: {_route('03')},
        translate: translate,
      );
      expect(plugin.cancelled.last, (id: third.id, tag: third.tag));
      await restarted.close();
    },
  );
}
