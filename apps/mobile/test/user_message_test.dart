import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/user_message.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/ui/theme.dart';

void main() {
  for (final scale in [1.0, 2.0]) {
    testWidgets('narrow messages at text scale $scale', (tester) async {
      tester.view.physicalSize = const Size(320, 900);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      var opened = false;
      const filename = 'a-long-reference-document-for-the-login-form.md';
      await tester.pumpWidget(
        MaterialApp(
          theme: SailryTheme.of(Brightness.dark),
          builder: (context, child) => MediaQuery(
            data: MediaQuery.of(
              context,
            ).copyWith(textScaler: TextScaler.linear(scale)),
            child: child!,
          ),
          home: Scaffold(
            body: ListView(
              padding: const EdgeInsets.symmetric(horizontal: 20),
              children: [
                const UserBubble(key: ValueKey('short'), text: 'OK'),
                UserBubble(
                  key: const ValueKey('long'),
                  text: 'Adjust the login form spacing and button styles',
                  attachment: SentAttachment(
                    name: filename,
                    onPressed: () => opened = true,
                  ),
                ),
              ],
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      Rect bubble(String key) => tester.getRect(
        find
            .descendant(
              of: find.byKey(ValueKey(key)),
              matching: find.byType(Material),
            )
            .first,
      );
      expect(bubble('short').right, closeTo(300, .1));
      expect(bubble('long').right, closeTo(300, .1));
      expect(bubble('short').width, lessThan(bubble('long').width));
      expect(bubble('long').width, lessThanOrEqualTo(280 * .88 + .1));
      final attachment = find.widgetWithText(OutlinedButton, filename);
      final bounds = tester.getRect(attachment);
      expect(bounds.left, greaterThan(bubble('long').left));
      expect(bounds.right, lessThan(bubble('long').right));
      expect(find.byType(SelectionArea), findsNWidgets(2));
      await tester.ensureVisible(attachment);
      await tester.tap(attachment);
      await tester.pumpAndSettle();
      expect(opened, isTrue);
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('message-specific copy and clipboard failure', (tester) async {
    String? copied;
    var fail = false;
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
      SystemChannels.platform,
      (call) async {
        if (call.method == 'Clipboard.setData') {
          if (fail) throw PlatformException(code: 'clipboard_unavailable');
          copied = (call.arguments as Map)['text'] as String;
        }
        return null;
      },
    );
    addTearDown(() {
      tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
        SystemChannels.platform,
        null,
      );
    });
    await tester.pumpWidget(
      const MaterialApp(
        home: Scaffold(
          body: Column(
            children: [
              UserBubble(key: ValueKey('first'), text: 'First message'),
              UserBubble(key: ValueKey('second'), text: 'Second message'),
            ],
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    Finder copy(String key) => find.descendant(
      of: find.byKey(ValueKey(key)),
      matching: find.byTooltip(tr('copy')),
    );
    await tester.tap(copy('first'));
    await tester.pumpAndSettle();
    expect(copied, 'First message');
    fail = true;
    await tester.tap(copy('second'));
    await tester.pumpAndSettle();
    expect(copied, 'First message');
    expect(find.text(tr('copyFailed')), findsOneWidget);
    expect(find.byType(SnackBar), findsNothing);
    await tester.pump(const Duration(seconds: 3));
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
  });
}
