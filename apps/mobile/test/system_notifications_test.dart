import 'package:flutter_local_notifications/flutter_local_notifications.dart';
import 'package:flutter_test/flutter_test.dart';
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

Map<String, dynamic> _notice(int entry, {bool read = false}) => {
  'id': {
    'node': List.filled(32, 9),
    'entry': List.filled(32, entry),
    'target': {'Session': 'session'},
  },
  'session': 'session',
  'kind': 'Completed',
  'read': read,
};

void main() {
  test(
    'restores and withdraws alerts by entry and target after restart',
    () async {
      final plugin = _NotificationPlugin();
      final alerts = SystemAlerts(supported: true, plugin: plugin);
      String translate(String key) => key;
      expect(await alerts.start(translate), isTrue);
      await alerts.sync(
        [_notice(1), _notice(2)],
        post: true,
        known: {},
        refreshedRoutes: {},
        translate: translate,
      );
      final [first, second] = plugin.shown;
      expect(first.id, isNot(second.id));
      final tap = alerts.taps.first;
      plugin.onTap!(
        NotificationResponse(
          notificationResponseType:
              NotificationResponseType.selectedNotification,
          payload: second.payload,
        ),
      );
      expect(await tap, (
        host: '${'02' * 32}/${'09' * 32}',
        session: 'session',
      ));
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
        post: false,
        known: {},
        refreshedRoutes: {},
        translate: translate,
      );
      expect(plugin.cancelled, isEmpty);
      await restarted.sync(
        [],
        post: false,
        known: {},
        refreshedRoutes: {'${'01' * 32}/${'09' * 32}'},
        translate: translate,
      );
      expect(plugin.cancelled, [(id: first.id, tag: first.tag)]);
      await restarted.sync(
        [_notice(2, read: true)],
        post: false,
        known: {},
        refreshedRoutes: {},
        translate: translate,
      );
      expect(plugin.cancelled, [
        (id: first.id, tag: first.tag),
        (id: second.id, tag: second.tag),
      ]);
      await restarted.close();
    },
  );
}
