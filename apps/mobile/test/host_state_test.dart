import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/tasks_page.dart';
import 'package:sailry_mobile/features/resources/live_hosts.dart';
import 'package:sailry_mobile/features/resources/live_resources.dart';
import 'package:sailry_mobile/features/settings/settings_page.dart';
import 'package:sailry_mobile/features/settings/usage_page.dart';
import 'package:sailry_mobile/features/settings/usage_watch.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/kit.dart';

import 'settings_test.dart' show pumpPage, tap;

class TestHost extends HostConnection {
  TestHost()
    : super.test(
        id: 'host',
        label: 'Host',
        connected: false,
        command: (_, _) async => {'data': <String, dynamic>{}},
      );

  void setConnected(bool value) {
    connected = value;
    notifyListeners();
  }
}

void main() {
  final pages = <String, Widget>{
    'tasks': const TasksPage(),
    'hosts': const LiveHostsPage(),
    'resources': const LiveResourcesPage(),
    'usage': const UsagePage(),
  };
  for (final entry in pages.entries) {
    for (final added in [false, true]) {
      testWidgets('${entry.key} distinguishes added=$added', (tester) async {
        final session = AppSession.test(hosts: added ? [TestHost()] : []);
        await pumpPage(tester, entry.value, session: session);
        expect(find.byType(HostState), findsOneWidget);
        expect(
          find.text(tr(added ? 'hostDisconnected' : 'hostConnectPrompt')),
          findsOneWidget,
        );
        expect(tester.takeException(), isNull);
      });
    }
  }

  testWidgets('Node settings follow connection changes', (tester) async {
    final host = TestHost();
    final session = AppSession.test(hosts: [host]);
    await pumpPage(
      tester,
      SettingsPage(themeMode: ThemeMode.light, onThemeChanged: (_) {}),
      session: session,
    );
    for (final connected in [false, true, false]) {
      host.setConnected(connected);
      await tester.pumpAndSettle();
      for (final key in ['providers', 'roles', 'memorySettings', 'usage']) {
        final row = tester.widget<ListTile>(
          find.widgetWithText(ListTile, tr(key)),
        );
        expect(row.enabled, connected);
        expect(row.onTap != null, connected);
      }
      expect(
        tester
            .widget<ListTile>(find.widgetWithText(ListTile, tr('appearance')))
            .enabled,
        isTrue,
      );
    }
    expect(tester.takeException(), isNull);
  });

  testWidgets('usage reconnects and keeps partial coverage in details', (
    tester,
  ) async {
    final host = TestHost();
    final session = AppSession.test(hosts: [host]);
    var opens = 0;
    var closes = 0;
    Future<UsageWatch> watch(
      AppSession _,
      HostConnection? _,
      Map<String, dynamic> _,
    ) async {
      opens++;
      var first = true;
      final stopped = Completer<String>();
      return UsageWatch(
        next: () async {
          if (!first) return stopped.future;
          first = false;
          return jsonEncode({
            'summary': {
              'totals': {'responses': 1},
            },
            'complete': false,
          });
        },
        close: () async {
          closes++;
          if (!stopped.isCompleted) stopped.completeError(StateError('closed'));
        },
      );
    }

    await pumpPage(tester, UsagePage(watch: watch), session: session);
    expect(opens, 0);
    host.setConnected(true);
    await tester.pumpAndSettle();
    expect(opens, 1);
    expect(find.byType(HostState), findsNothing);
    expect(find.text(tr('settingsUsagePartial')), findsNothing);
    await tap(tester, find.byTooltip(tr('details')));
    expect(find.text(tr('settingsUsagePartial')), findsOneWidget);
    Navigator.pop(tester.element(find.byType(BottomSheet)));
    await tester.pumpAndSettle();
    host.setConnected(false);
    await tester.pumpAndSettle();
    expect(closes, 1);
    expect(find.text(tr('hostDisconnected')), findsOneWidget);
    expect(find.text(tr('settingsUsagePartial')), findsNothing);
    expect(tester.takeException(), isNull);
  });
}
