import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/connection_status.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/ui/connection_status.dart';
import 'package:sailry_mobile/ui/theme.dart';

const _reason = 'relay closed: 503';

Future<void> _show(
  WidgetTester tester,
  Widget child, {
  bool disableAnimations = false,
}) => tester.pumpWidget(
  MaterialApp(
    locale: const Locale('en'),
    theme: SailryTheme.of(Brightness.light),
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    home: MediaQuery(
      data: MediaQueryData(disableAnimations: disableAnimations),
      child: Scaffold(body: child),
    ),
  ),
);

String _en(WidgetTester tester, String key) =>
    tester.element(find.byType(Scaffold)).tr(key);

Future<void> _expectDetailsSheet(WidgetTester tester) async {
  await tester.pumpAndSettle();
  expect(find.widgetWithText(SelectableText, _reason), findsOneWidget);
  final copied = <String>[];
  tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
    SystemChannels.platform,
    (call) async {
      if (call.method == 'Clipboard.setData') {
        copied.add((call.arguments as Map)['text'] as String);
      }
      return null;
    },
  );
  addTearDown(
    () => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
      SystemChannels.platform,
      null,
    ),
  );
  await tester.tap(find.byTooltip(_en(tester, 'copy')));
  await tester.pump();
  expect(copied, [_reason]);
  await tester.pump(const Duration(seconds: 3));
}

bool _live(WidgetTester tester, Finder finder) => tester
    .widgetList<Semantics>(
      find.ancestor(of: finder, matching: find.byType(Semantics)),
    )
    .any((semantics) => semantics.properties.liveRegion == true);

void main() {
  testWidgets('banner retries, opens details and animates in and out', (
    tester,
  ) async {
    var retries = 0;
    Widget banner({required bool visible, bool reconnecting = false}) =>
        ConnectionBanner(
          visible: visible,
          reconnecting: reconnecting,
          reason: _reason,
          onRetry: () => retries++,
        );
    await _show(tester, banner(visible: false));
    final line = find.text(_en(tester, 'conversationUnavailable'));
    expect(line, findsNothing);

    await _show(tester, banner(visible: true));
    await tester.pump(const Duration(milliseconds: 50));
    expect(line, findsOneWidget);
    expect(tester.hasRunningAnimations, isTrue);
    await tester.pumpAndSettle();
    expect(_live(tester, line), isTrue);
    expect(find.byType(CircularProgressIndicator), findsNothing);
    await tester.tap(find.widgetWithText(TextButton, _en(tester, 'retry')));
    expect(retries, 1);

    await tester.tap(line);
    await _expectDetailsSheet(tester);
    await tester.tapAt(const Offset(4, 4));
    await tester.pumpAndSettle();

    // The spinner never settles, so these steps pump fixed durations.
    await _show(tester, banner(visible: true, reconnecting: true));
    expect(find.text(_en(tester, 'conversationReconnecting')), findsOneWidget);
    expect(find.byType(CircularProgressIndicator), findsOneWidget);
    expect(find.text(_en(tester, 'retry')), findsNothing);

    await _show(tester, banner(visible: false));
    await tester.pump(const Duration(milliseconds: 100));
    expect(find.text(_en(tester, 'conversationReconnecting')), findsOneWidget);
    await tester.pump(const Duration(milliseconds: 200));
    expect(find.text(_en(tester, 'conversationReconnecting')), findsNothing);
  });

  testWidgets('banner skips its transition when animations are disabled', (
    tester,
  ) async {
    await _show(
      tester,
      const ConnectionBanner(visible: false, reconnecting: false),
      disableAnimations: true,
    );
    await _show(
      tester,
      const ConnectionBanner(visible: true, reconnecting: false),
      disableAnimations: true,
    );
    await tester.pump();
    final line = find.text(_en(tester, 'conversationUnavailable'));
    expect(line, findsOneWidget);
    expect(
      tester
          .widget<FadeTransition>(
            find
                .ancestor(of: line, matching: find.byType(FadeTransition))
                .first,
          )
          .opacity
          .value,
      1,
    );
  });

  testWidgets('empty state retries or shows progress, with details', (
    tester,
  ) async {
    var retries = 0;
    await _show(
      tester,
      DisconnectedState(
        reconnecting: false,
        reason: _reason,
        onRetry: () => retries++,
      ),
    );
    final title = find.text(_en(tester, 'conversationUnavailable'));
    expect(title, findsOneWidget);
    expect(_live(tester, title), isTrue);
    expect(
      find.text(_en(tester, 'conversationUnavailableHint')),
      findsOneWidget,
    );
    await tester.tap(find.widgetWithText(FilledButton, _en(tester, 'retry')));
    expect(retries, 1);
    await tester.tap(find.text(_en(tester, 'connectionDetails')));
    await _expectDetailsSheet(tester);
    await tester.tapAt(const Offset(4, 4));
    await tester.pumpAndSettle();

    await _show(
      tester,
      const DisconnectedState(reconnecting: true, reason: _reason),
    );
    expect(find.text(_en(tester, 'conversationReconnecting')), findsOneWidget);
    expect(
      find.text(_en(tester, 'conversationReconnectingHint')),
      findsOneWidget,
    );
    expect(find.byType(CircularProgressIndicator), findsOneWidget);
    expect(find.byType(FilledButton), findsNothing);
    expect(find.text(_en(tester, 'connectionDetails')), findsOneWidget);

    await _show(tester, const DisconnectedState(reconnecting: false));
    expect(find.byType(FilledButton), findsNothing);
    expect(find.text(_en(tester, 'connectionDetails')), findsNothing);
  });

  test("separates routes and observed latency through disconnect", () {
    for (final (caseName, run) in <(String, void Function(String))>[
      (
        "selected route and RTT remain separate",
        (caseName) {
          for (final route in ['p2p', 'relay']) {
            final columns = connectionColumns(true, {
              'route': route,
              'rtt_ms': 1248,
            });
            expect(
              columns.$1,
              route == 'p2p' ? 'connectionRouteP2p' : 'connectionRouteRelay',
              reason: caseName,
            );
            expect(columns.$2, '1248 ms', reason: caseName);
          }
        },
      ),
      (
        "disconnect and unknown routes discard stale RTT",
        (caseName) {
          expect(connectionColumns(false, {'route': 'relay', 'rtt_ms': 42}), (
            'connectionRouteUnknown',
            null,
          ), reason: caseName);
          expect(connectionColumns(true, {'rtt_ms': 42}), (
            'connectionRouteUnknown',
            null,
          ), reason: caseName);
          expect(connectionColumns(true, {'route': 'local', 'rtt_ms': 0}), (
            'connectionRouteUnknown',
            null,
          ), reason: caseName);
          expect(connectionColumns(true, {'route': 'p2p', 'rtt_ms': -1}), (
            'connectionRouteP2p',
            null,
          ), reason: caseName);
        },
      ),
    ]) {
      run(caseName);
    }
  });
}
