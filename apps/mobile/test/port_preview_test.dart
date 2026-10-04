import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/resources/port_preview.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/ui/kit.dart';
import 'package:webview_flutter/webview_flutter.dart';

import 'support/webview.dart';

void main() {
  late BrowserPlatform browser;
  late ValueNotifier<bool> listening;
  final uri = Uri.parse('http://127.0.0.1:43123');

  setUp(() {
    browser = BrowserPlatform();
    WebViewPlatform.instance = browser;
    listening = ValueNotifier(true);
  });
  tearDown(() => listening.dispose());

  Future<void> mount(WidgetTester tester) async {
    await tester.pumpWidget(
      MaterialApp(
        home: PortPreviewPage(uri: uri, listening: listening),
      ),
    );
    await tester.pump();
  }

  testWidgets('loads once and refreshes only on request', (tester) async {
    await mount(tester);
    expect(browser.controller.requests, [uri]);
    expect(browser.controller.javascript, JavaScriptMode.unrestricted);
    await tester.tap(find.byTooltip(tr('refresh')));
    expect(browser.controller.reloads, 0);
    browser.navigation.finished(uri.toString());
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip(tr('refresh')));
    await tester.tap(find.byTooltip(tr('refresh')));
    await tester.pump();
    expect(browser.controller.reloads, 1);
    browser.navigation.finished(uri.toString());
    await tester.pumpAndSettle();
    expect(browser.controller.requests, [uri]);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('distinguishes page errors from subresource failures', (
    tester,
  ) async {
    await mount(tester);
    browser.navigation.error(
      const WebResourceError(
        errorCode: -1,
        description: 'Missing asset',
        isForMainFrame: false,
      ),
    );
    browser.navigation.finished(uri.toString());
    await tester.pumpAndSettle();
    expect(find.byType(FailureState), findsNothing);
    browser.navigation.error(
      const WebResourceError(
        errorCode: -2,
        description: 'Disconnected',
        isForMainFrame: true,
      ),
    );
    browser.navigation.finished(uri.toString());
    await tester.pumpAndSettle();
    expect(find.text(tr('resourcePreviewFailed')), findsOneWidget);
    await tester.tap(find.text(tr('retry')));
    await tester.pump();
    expect(browser.controller.reloads, 1);
    listening.value = false;
    await tester.pumpAndSettle();
    expect(find.text(tr('resourceForwardStopped')), findsOneWidget);
    expect(find.byType(WebViewWidget), findsNothing);
    expect(find.text(tr('retry')), findsNothing);
    await tester.tap(find.byTooltip(tr('refresh')));
    expect(browser.controller.reloads, 1);
    await tester.pumpWidget(const SizedBox());
    browser.navigation.finished(uri.toString());
    expect(tester.takeException(), isNull);
  });

  testWidgets('keeps web navigation inside the view without native schemes', (
    tester,
  ) async {
    await mount(tester);
    expect(
      await browser.navigation.request(
        const NavigationRequest(
          url: 'https://example.com/page',
          isMainFrame: true,
        ),
      ),
      NavigationDecision.navigate,
    );
    expect(
      await browser.navigation.request(
        const NavigationRequest(
          url: 'file:///private/example',
          isMainFrame: true,
        ),
      ),
      NavigationDecision.prevent,
    );
    await tester.pump();
    expect(find.text(tr('resourcePreviewLink')), findsOneWidget);
    expect(browser.controller.requests, [uri]);
    await tester.pumpWidget(const SizedBox());
  });
}
