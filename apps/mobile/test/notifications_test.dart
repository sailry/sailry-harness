import 'dart:async';
import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_bridge/api.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:sailry_mobile/app.dart';
import 'package:sailry_mobile/features/notifications/delivery.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/theme.dart';

Map<String, dynamic> notice(
  int cursor,
  String title, {
  String kind = 'Completed',
  String target = 'Session',
}) => {
  'id': {
    'node': List.filled(32, 1),
    'cursor': cursor,
    'target': {target: 'resource'},
  },
  'title': title,
  'kind': kind,
  'read': false,
};

class Inbox extends Fake implements Controller {
  final records = <Map<String, dynamic>>[];
  @override
  Future<String> notifications() async => jsonEncode({'notices': records});
  @override
  Future<void> close() async {}
  @override
  void dispose() {}
}

class Host extends HostConnection {
  Host()
    : super.test(
        id: 'node',
        label: 'Node',
        snapshot: {},
        command: (kind, data) async => {},
      );
  void changed() => notifyListeners();
}

class Preferences extends Fake implements SharedPreferencesAsync {
  final values = <String, Object>{};
  Future<bool?>? get pending => values['pending'] as Future<bool?>?;
  set pending(Future<bool?>? value) {
    if (value == null) {
      values.remove('pending');
    } else {
      values['pending'] = value;
    }
  }

  @override
  Future<bool?> getBool(String key) async => pending ?? values[key] as bool?;
  @override
  Future<String?> getString(String key) async => values[key] as String?;
  @override
  Future<void> setBool(String key, bool value) async {
    values[key] = value;
  }

  @override
  Future<void> setString(String key, String value) async {
    values[key] = value;
  }
}

Future<void> mount(
  WidgetTester tester,
  AppSession session, {
  bool enabled = true,
}) async {
  await tester.pumpWidget(
    MaterialApp(
      theme: SailryTheme.of(Brightness.light),
      home: NotificationDelivery(
        session: session,
        enabled: enabled,
        child: const Scaffold(body: Text('Task')),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('completion toast dismisses in place', (tester) async {
    final inbox = Inbox()..records.add(notice(1, 'Historical task'));
    final host = Host();
    final session = AppSession.test(hosts: [host], controller: inbox);
    await mount(tester, session);
    final toast = find.byKey(const ValueKey('completion-toast'));
    expect(toast, findsNothing);
    inbox.records.insert(0, notice(2, 'New task'));
    host.changed();
    await tester.pumpAndSettle();
    expect(toast, findsOneWidget);
    expect(find.text('New task · ${tr('completed')}'), findsOneWidget);
    expect(find.byType(SnackBar), findsNothing);
    expect(find.byTooltip(tr('notification')), findsNothing);
    await tester.pump(const Duration(seconds: 3));
    await tester.pumpAndSettle();
    expect(toast, findsNothing);
    host.changed();
    await tester.pumpAndSettle();
    expect(toast, findsNothing);
    expect(inbox.records.every((n) => n['read'] == false), isTrue);
    await tester.pumpWidget(const SizedBox());
    await session.close();
    session.dispose();
  });

  testWidgets('filters disabled, background, and non-session events', (
    tester,
  ) async {
    final inbox = Inbox();
    final host = Host();
    final session = AppSession.test(hosts: [host], controller: inbox);
    final toast = find.byKey(const ValueKey('completion-toast'));
    await mount(tester, session, enabled: false);
    inbox.records.add(notice(1, 'Quiet task'));
    host.changed();
    await tester.pumpAndSettle();
    await mount(tester, session);
    host.changed();
    await tester.pumpAndSettle();
    expect(toast, findsNothing);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
    inbox.records.add(notice(2, 'Background task'));
    host.changed();
    await tester.pump();
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await tester.pumpAndSettle();
    expect(toast, findsNothing);
    inbox.records.addAll([
      notice(3, 'Terminal', target: 'Terminal'),
      notice(4, 'Approval', kind: 'Approval'),
    ]);
    host.changed();
    await tester.pumpAndSettle();
    expect(toast, findsNothing);
    inbox.records.add(notice(5, 'Next task'));
    host.changed();
    await tester.pumpAndSettle();
    expect(toast, findsOneWidget);
    await tester.pumpWidget(const SizedBox());
    await session.close();
    session.dispose();
  });

  testWidgets('saved preference resists stale reads', (tester) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(320, 740);
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final preferences = Preferences();
    final pending = Completer<bool?>();
    preferences.pending = pending.future;
    final session = AppSession.test(controller: Inbox());
    await tester.pumpWidget(
      SailryApp(session: session, preferences: preferences),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('tab-3')));
    await tester.pumpAndSettle();
    final toggle = find.widgetWithText(SwitchListTile, tr('completionAlerts'));
    await tester.tap(toggle);
    await tester.pumpAndSettle();
    expect(preferences.values['notifications.in_app'], isFalse);
    pending.complete(true);
    await tester.pumpAndSettle();
    expect(tester.widget<SwitchListTile>(toggle).value, isFalse);
    preferences.pending = null;
    await tester.pumpWidget(const SizedBox());
    await tester.pumpWidget(
      SailryApp(session: session, preferences: preferences),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('tab-3')));
    await tester.pumpAndSettle();
    expect(tester.widget<SwitchListTile>(toggle).value, isFalse);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    await session.close();
    session.dispose();
  });
}
