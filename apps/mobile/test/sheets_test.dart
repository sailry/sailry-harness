import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/ui/kit.dart';
import 'package:sailry_mobile/ui/theme.dart';

Future<void> mountSheet(
  WidgetTester tester, {
  required double width,
  required Widget child,
  ValueNotifier<double>? keyboard,
  List<Widget> actions = const [],
  Brightness brightness = Brightness.light,
}) async {
  tester.view.devicePixelRatio = 1;
  tester.view.physicalSize = Size(width, 844);
  addTearDown(tester.view.resetDevicePixelRatio);
  addTearDown(tester.view.resetPhysicalSize);
  final inset = keyboard ?? ValueNotifier<double>(0);
  if (keyboard == null) addTearDown(inset.dispose);
  await tester.pumpWidget(
    MaterialApp(
      theme: SailryTheme.of(brightness),
      builder: (context, child) => ValueListenableBuilder<double>(
        valueListenable: inset,
        builder: (context, value, _) => MediaQuery(
          data: MediaQuery.of(context).copyWith(
            padding: EdgeInsets.only(top: 44, bottom: value == 0 ? 34 : 0),
            viewPadding: const EdgeInsets.only(top: 44, bottom: 34),
            viewInsets: EdgeInsets.only(bottom: value),
          ),
          child: child!,
        ),
      ),
      home: Builder(
        builder: (context) => Scaffold(
          body: Center(
            child: TextButton(
              onPressed: () => showAppSheet<void>(
                context,
                'Worktree',
                actions: actions,
                child: child,
              ),
              child: const Text('Open'),
            ),
          ),
        ),
      ),
    ),
  );
  await tester.tap(find.text('Open'));
  await tester.pumpAndSettle();
}

Finder sheetSurface() => find.byWidgetPredicate(
  (widget) => widget is Surface && widget.kind == SurfaceKind.sheet,
);

void main() {
  for (final brightness in Brightness.values) {
    testWidgets('sheet and confirmation use tinted glass in $brightness', (
      tester,
    ) async {
      await mountSheet(
        tester,
        width: 320,
        brightness: brightness,
        child: Builder(
          builder: (context) => TextButton(
            onPressed: () => showAppDialog<void>(
              context: context,
              builder: (context) => AlertDialog(
                title: const Text('Confirm action'),
                content: const Text('Confirmation content'),
                actions: [
                  TextButton(
                    onPressed: () => Navigator.pop(context),
                    child: const Text('Cancel'),
                  ),
                ],
              ),
            ),
            child: const Text('Confirm'),
          ),
        ),
      );
      final panel = tester.getRect(sheetSurface());
      final ink = tester.widget<Ink>(
        find.descendant(of: sheetSurface(), matching: find.byType(Ink)).first,
      );
      final fill = (ink.decoration! as BoxDecoration).gradient!;
      expect(
        fill.colors.every((color) => color.a > .65 && color.a < .9),
        isTrue,
      );
      final sheetBlur = find.descendant(
        of: sheetSurface(),
        matching: find.byType(BackdropFilter),
      );
      expect(sheetBlur, findsOneWidget);
      expect(tester.widget<BackdropFilter>(sheetBlur).enabled, isTrue);
      await tester.tap(find.text('Confirm'));
      await tester.pumpAndSettle();
      final dialog = tester.widget<Material>(
        find
            .descendant(
              of: find.byType(Dialog),
              matching: find.byType(Material),
            )
            .first,
      );
      expect(dialog.color?.a, inExclusiveRange(.65, .9));
      final dialogBlur = find.ancestor(
        of: find.byType(AlertDialog),
        matching: find.byType(BackdropFilter),
      );
      expect(dialogBlur, findsOneWidget);
      expect(tester.widget<BackdropFilter>(dialogBlur).enabled, isTrue);
      await tester.tap(find.text('Cancel'));
      await tester.pumpAndSettle();
      expect(find.byType(AlertDialog), findsNothing);
      expect(tester.getRect(sheetSurface()), panel);
      expect(tester.takeException(), isNull);
    });
  }

  for (final width in [320.0, 390.0]) {
    testWidgets('inset worktree rows at width $width', (tester) async {
      var taps = 0;
      var actionTaps = 0;
      const rowKey = ValueKey('worktree-row');
      await mountSheet(
        tester,
        width: width,
        actions: [RoundButton(icon: 'plus', onPressed: () => actionTaps++)],
        child: Surface(
          key: rowKey,
          onTap: () => taps++,
          child: const Text('feature/sign-in'),
        ),
      );

      final panel = tester.getRect(sheetSurface());
      final row = tester.getRect(find.byKey(rowKey));
      expect(panel.left, 12);
      expect(panel.right, width - 12);
      expect(panel.bottom, 844 - 34 - 12);
      expect(row.left, panel.left + 20);
      expect(row.right, panel.right - 20);
      await tester.tapAt(Offset(row.right - 8, row.center.dy));
      await tester.pump();
      expect(taps, 1);

      await tester.tap(
        find.byWidgetPredicate(
          (widget) => widget is RoundButton && widget.icon == 'plus',
        ),
      );
      await tester.pump();
      expect(actionTaps, 1);
      expect(find.text('Worktree'), findsNothing);
      expect(find.byTooltip(tr('close')), findsNothing);
      await tester.tapAt(const Offset(8, 80));
      await tester.pumpAndSettle();
      expect(sheetSurface(), findsNothing);
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('handle drag dismisses the sheet', (tester) async {
    await mountSheet(tester, width: 390, child: const Text('Sheet content'));
    final panel = tester.getRect(sheetSurface());
    await tester.flingFrom(
      Offset(panel.center.dx, panel.top + 12),
      const Offset(0, 350),
      1200,
    );
    await tester.pumpAndSettle();
    expect(sheetSurface(), findsNothing);
  });

  testWidgets('centered sheets respect native width limits', (tester) async {
    await mountSheet(
      tester,
      width: 900,
      child: const Surface(child: Text('feature/sign-in')),
    );

    final panel = tester.getRect(sheetSurface());
    expect(panel.center.dx, 450);
    expect(panel.width, lessThanOrEqualTo(520));
    expect(
      tester.widget<BottomSheet>(find.byType(BottomSheet)).constraints,
      const BoxConstraints(maxWidth: 520),
    );
    expect(tester.takeException(), isNull);
  });

  testWidgets('scrolling content adapts to the keyboard', (tester) async {
    final keyboard = ValueNotifier<double>(0);
    addTearDown(keyboard.dispose);
    var taps = 0;
    await mountSheet(
      tester,
      width: 320,
      keyboard: keyboard,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const TextField(),
          for (var index = 0; index < 15; index++)
            ListTile(title: Text('Branch $index')),
          TextButton(onPressed: () => taps++, child: const Text('Select')),
        ],
      ),
    );

    for (final inset in [300.0, 0.0]) {
      keyboard.value = inset;
      await tester.pumpAndSettle();
      final panel = tester.getRect(sheetSurface());
      final bottom = 844 - (inset == 0 ? 34 : inset) - 12;
      expect(panel.bottom, bottom);
      expect(panel.top, greaterThanOrEqualTo(44));
      expect(panel.height, lessThanOrEqualTo((bottom - 44) * .78 + .01));
      await tester.ensureVisible(find.text('Select'));
      await tester.pumpAndSettle();
      final button = tester.getRect(find.widgetWithText(TextButton, 'Select'));
      expect(button.bottom, lessThanOrEqualTo(panel.bottom));
      await tester.tap(find.text('Select'));
      await tester.pump();
      expect(tester.takeException(), isNull);
    }
    expect(taps, 2);
  });
}
