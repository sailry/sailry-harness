import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/host_status_bar.dart';
import 'package:sailry_mobile/features/conversations/live/tasks.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';

HostConnection host(
  String label, {
  bool connected = true,
  String? route,
  num? rtt,
  Object? error,
}) =>
    HostConnection.test(
        id: label,
        label: label,
        connected: connected,
        snapshot: {
          'projects': [
            {'id': 'project', 'name': '$label project', 'path': '/$label'},
          ],
          'sessions': [
            {
              'id': '$label-session',
              'revision': 1,
              'activity': {'title': '$label task'},
            },
          ],
        },
      )
      ..transport = {'route': ?route, 'rtt_ms': ?rtt}
      ..error = error;

Future<AppSession> show(
  WidgetTester tester,
  List<HostConnection> hosts, {
  bool settle = true,
}) async {
  final session = AppSession.test(hosts: hosts);
  addTearDown(() async {
    await tester.pumpWidget(const SizedBox.shrink());
    await session.close();
    session.dispose();
  });
  await tester.pumpWidget(
    SessionScope(
      session: session,
      child: MaterialApp(
        locale: const Locale('en'),
        supportedLocales: AppLocalizations.supportedLocales,
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        home: LiveTasksPage(session: session),
      ),
    ),
  );
  settle ? await tester.pumpAndSettle() : await tester.pump();
  return session;
}

Finder bar(String text) =>
    find.descendant(of: find.byType(HostStatusBar), matching: find.text(text));

String label(WidgetTester tester) =>
    tester.getSemantics(find.byType(HostStatusBar)).label;

void main() {
  for (final (route, name) in [('p2p', 'P2P'), ('relay', 'Relay')]) {
    testWidgets('connected host shows $name route and latency', (tester) async {
      await show(tester, [host('Studio', route: route, rtt: 23.4)]);
      // Names stay in the semantic label, not on the line.
      expect(bar('Studio'), findsNothing);
      // The green dot shows the state; screen readers still hear it.
      expect(bar('Connected'), findsNothing);
      expect(bar(name), findsOneWidget);
      expect(bar('23 ms'), findsOneWidget);
      expect(label(tester), 'Studio, Connected, $name, 23 ms');
    });
  }

  testWidgets('host without a view or fault is connecting', (tester) async {
    await show(tester, [host('Studio', connected: false)], settle: false);
    expect(bar('Connecting'), findsOneWidget);
    expect(bar('P2P'), findsNothing);
    expect(find.textContaining(' ms'), findsNothing);
  });

  testWidgets('host with a fault is disconnected', (tester) async {
    await show(tester, [
      host('Studio', connected: false, route: 'p2p', rtt: 5, error: 'closed'),
    ]);
    expect(bar('Disconnected'), findsOneWidget);
    expect(bar('P2P'), findsNothing);
    expect(label(tester), 'Studio, Disconnected');
  });

  testWidgets('all projects summarize hosts with the fastest route', (
    tester,
  ) async {
    await show(tester, [
      host('Studio', route: 'relay', rtt: 80),
      host('Laptop', route: 'p2p', rtt: 12),
      host('Server', connected: false, error: 'closed'),
    ]);
    // Names stay in the semantic label, not on the line.
    expect(bar('2/3 hosts connected'), findsNothing);
    expect(bar('P2P'), findsOneWidget);
    expect(bar('12 ms'), findsOneWidget);
    expect(label(tester), '2/3 hosts connected, P2P, 12 ms');
  });

  testWidgets('a selected project shows only its host', (tester) async {
    final session = await show(tester, [
      host('Studio', route: 'relay', rtt: 80),
      host('Laptop', route: 'p2p', rtt: 12),
    ]);
    session.selectProject('Studio', 'project');
    await tester.pumpAndSettle();
    // Names stay in the semantic label, not on the line.
    expect(bar('Studio'), findsNothing);
    expect(bar('Relay'), findsOneWidget);
    expect(bar('80 ms'), findsOneWidget);
  });

  testWidgets('tapping the bar opens the host sheet', (tester) async {
    await show(tester, [host('Studio', route: 'p2p', rtt: 23)]);
    await tester.tap(find.byType(HostStatusBar));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('host-row-Studio')), findsOneWidget);
  });

  testWidgets('no hosts hide the bar', (tester) async {
    await show(tester, []);
    expect(find.byType(LiveTasksPage), findsOneWidget);
    expect(find.byType(HostStatusBar), findsNothing);
  });
}
