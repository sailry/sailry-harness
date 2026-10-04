import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/app.dart';
import 'package:sailry_mobile/features/resources/live_hosts.dart';
import 'package:sailry_mobile/features/settings/setting_records.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/kit.dart';
import 'package:sailry_mobile/ui/theme.dart';

void viewport(WidgetTester tester, {Size size = const Size(390, 844)}) {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
}

class StartupSession extends AppSession {
  StartupSession() : super.test(ready: false) {
    error = StateError('Initialization failed');
  }

  int retries = 0;

  @override
  Future<void> start({String? path, bool internet = true}) async {
    retries++;
    ready = true;
    error = null;
    notifyListeners();
  }
}

void main() {
  for (final brightness in Brightness.values) {
    testWidgets('inverse retry colors in $brightness', (tester) async {
      await tester.pumpWidget(
        MaterialApp(
          theme: SailryTheme.of(brightness),
          home: Scaffold(
            body: FailureState(message: 'Failed', onRetry: () {}),
          ),
        ),
      );
      final finder = find.widgetWithText(FilledButton, tr('retry'));
      final button = tester.widget<FilledButton>(finder);
      final context = tester.element(finder);
      final colors = Theme.of(context).colorScheme;
      final style = button.defaultStyleOf(context);
      expect(style.backgroundColor?.resolve({}), colors.primary);
      expect(style.foregroundColor?.resolve({}), colors.onPrimary);
    });
  }

  testWidgets('replaces stale content and fills the body', (tester) async {
    viewport(tester);
    await tester.pumpWidget(
      MaterialApp(
        theme: SailryTheme.of(Brightness.dark),
        home: const PageFrame(
          title: 'Files',
          empty: EmptyState(message: 'No files'),
          failure: FailureState(icon: 'folder', message: 'Load failed'),
          child: Text('Stale files'),
        ),
      ),
    );
    await tester.pumpAndSettle();
    final failure = find.byType(FailureState);
    final bounds = tester.getRect(failure);
    final icon = tester.getRect(
      find.descendant(of: failure, matching: find.byType(AppIcon)),
    );
    final text = tester.getRect(find.text('Load failed'));
    expect(bounds.height, greaterThan(700));
    expect(bounds.bottom, 844 - 24);
    expect((icon.top + text.bottom) / 2, closeTo(bounds.center.dy, .01));
    expect(icon.center.dx, closeTo(text.center.dx, .01));
    expect(icon.bottom, lessThan(text.top));
    expect(find.text('Stale files'), findsNothing);
    expect(find.text('No files'), findsNothing);
    expect(find.byType(Surface), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('reachable retry with large text and short screen', (
    tester,
  ) async {
    viewport(tester, size: const Size(320, 300));
    var retries = 0;
    await tester.pumpWidget(
      MaterialApp(
        builder: (context, child) => MediaQuery(
          data: MediaQuery.of(
            context,
          ).copyWith(textScaler: TextScaler.linear(2)),
          child: child!,
        ),
        home: PageFrame(
          title: 'Files',
          failure: FailureState(
            message: 'The host could not be reached',
            onRetry: () => retries++,
          ),
          child: const SizedBox(),
        ),
      ),
    );
    await tester.pumpAndSettle();
    await tester.ensureVisible(find.text(tr('retry')));
    await tester.pumpAndSettle();
    await tester.tap(find.text(tr('retry')));
    expect(retries, 1);
    expect(tester.takeException(), isNull);
  });

  testWidgets('settings sheet recovers on retry', (tester) async {
    viewport(tester);
    var fail = true;
    var reads = 0;
    final host = HostConnection.test(
      id: 'node',
      label: 'Host',
      command: (kind, _) async {
        expect(kind, 'list_roles');
        reads++;
        if (fail) throw const CommandFailure('unavailable');
        return {'data': <Object>[]};
      },
    );
    await tester.pumpWidget(
      MaterialApp(
        theme: SailryTheme.of(Brightness.dark),
        home: Scaffold(
          body: Builder(
            builder: (context) => TextButton(
              onPressed: () =>
                  showSettingRecords(context, kind: 'roles', host: host),
              child: const Text('Open'),
            ),
          ),
        ),
      ),
    );
    await tester.tap(find.text('Open'));
    await tester.pumpAndSettle();
    final failure = find.byType(FailureState);
    expect(failure, findsOneWidget);
    final bounds = tester.getRect(failure);
    expect(bounds.height, greaterThan(400));
    final icon = tester.getRect(
      find.descendant(of: failure, matching: find.byType(AppIcon)),
    );
    final retry = tester.getRect(
      find.widgetWithText(FilledButton, tr('retry')),
    );
    expect((icon.top + retry.bottom) / 2, closeTo(bounds.center.dy, .01));
    fail = false;
    await tester.tap(find.text(tr('retry')));
    await tester.pumpAndSettle();
    expect(reads, 2);
    expect(find.byType(FailureState), findsNothing);
    expect(find.text(tr('settingsEmpty')), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    host.dispose();
  });

  testWidgets('single startup recovery before tab data', (tester) async {
    viewport(tester);
    final session = StartupSession();
    await tester.pumpWidget(SailryApp(session: session));
    await tester.pumpAndSettle();
    expect(find.byType(FailureState), findsOneWidget);
    expect(find.text(tr('startupFailed')), findsOneWidget);
    expect(find.text(tr('conversationNoHost')), findsNothing);
    await tester.tap(find.text(tr('retry')));
    await tester.pumpAndSettle();
    expect(session.retries, 1);
    expect(find.byType(FailureState), findsNothing);
    expect(find.text(tr('conversationNoHost')), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    session.dispose();
  });

  for (final connected in <bool?>[null, false, true]) {
    testWidgets('distinct resume error when connected=$connected', (
      tester,
    ) async {
      viewport(tester);
      final host = connected == null
          ? null
          : HostConnection.test(
              id: 'node',
              label: 'Host',
              connected: connected,
              command: (_, _) async => {'data': <String, dynamic>{}},
            );
      final session = AppSession.test(hosts: [?host])
        ..error = StateError('Network changed');
      await tester.pumpWidget(
        SessionScope(
          session: session,
          child: MaterialApp(
            theme: SailryTheme.of(Brightness.dark),
            home: const LiveHostsPage(),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.text(tr('startupFailed')), findsNothing);
      expect(
        find.byType(FailureState),
        connected == false ? findsOneWidget : findsNothing,
      );
      expect(
        find.text(tr('resourceDisconnected')),
        connected == false ? findsOneWidget : findsNothing,
      );
      if (host == null) expect(find.text(tr('connectFirst')), findsOneWidget);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
      session.dispose();
    });
  }
}
