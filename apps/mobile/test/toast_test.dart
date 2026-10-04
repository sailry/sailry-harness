import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/app.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/theme.dart';
import 'package:sailry_mobile/ui/toast.dart';

import 'notifications_test.dart' show Preferences;

class UnavailablePreferences extends Preferences {
  @override
  Future<String?> getString(String key) async =>
      throw StateError('Unavailable');
}

void main() {
  testWidgets('offline actions report in place', (tester) async {
    final session = AppSession.test(hosts: []);
    await tester.pumpWidget(SailryApp(session: session));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip(tr('newTask')));
    await tester.pumpAndSettle();
    final toast = find.byKey(const ValueKey('app-toast'));
    expect(
      find.descendant(of: toast, matching: find.text(tr('conversationNoHost'))),
      findsOneWidget,
    );
    expect(find.byType(SnackBar), findsNothing);
    expect(find.byType(BottomSheet), findsNothing);
    expect(tester.getSize(toast).width, lessThan(300));
    await tester.pump(const Duration(seconds: 3));
    await tester.pumpAndSettle();
    expect(toast, findsNothing);
    await tester.pumpWidget(const SizedBox());
    session.dispose();
  });

  testWidgets('preference failures use the shared overlay', (tester) async {
    final session = AppSession.test(hosts: []);
    await tester.pumpWidget(
      SailryApp(session: session, preferences: UnavailablePreferences()),
    );
    await tester.pumpAndSettle();
    expect(find.text(tr('preferencesFailed')), findsOneWidget);
    expect(find.byType(SnackBar), findsNothing);
    await tester.pumpWidget(const SizedBox());
    session.dispose();
  });

  testWidgets('replacement feedback stays above the keyboard', (tester) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(390, 844);
    tester.view.viewInsets = const FakeViewPadding(bottom: 280);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetViewInsets);
    var taps = 0;
    late BuildContext pageContext;
    await tester.pumpWidget(
      MaterialApp(
        theme: SailryTheme.of(Brightness.dark),
        home: Builder(
          builder: (context) {
            pageContext = context;
            return Scaffold(
              body: GestureDetector(
                behavior: HitTestBehavior.opaque,
                onTap: () => taps++,
                child: const SizedBox.expand(),
              ),
            );
          },
        ),
      ),
    );
    showToast(pageContext, 'First');
    await tester.pumpAndSettle();
    showToast(pageContext, 'Second');
    await tester.pumpAndSettle();
    expect(find.text('First'), findsNothing);
    expect(find.text('Second'), findsOneWidget);
    final bounds = tester.getRect(find.byKey(const ValueKey('app-toast')));
    expect(bounds.bottom, lessThan(844 - 280));
    await tester.tapAt(bounds.center);
    expect(taps, 1);
    await tester.pump(const Duration(seconds: 3));
    await tester.pumpAndSettle();
    expect(find.text('Second'), findsNothing);
  });
}
