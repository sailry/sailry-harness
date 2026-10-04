import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/ui/kit.dart';
import 'package:sailry_mobile/ui/theme.dart';
import 'package:sailry_mobile/ui/app_background.dart';

void main() {
  testWidgets('route transitions cover the previous page', (tester) async {
    await tester.pumpWidget(
      MaterialApp(
        theme: SailryTheme.of(Brightness.dark),
        home: Builder(
          builder: (context) => TextButton(
            onPressed: () => pushPage(
              context,
              const PageFrame(title: 'Detail', child: SizedBox()),
            ),
            child: const Text('Open'),
          ),
        ),
      ),
    );
    await tester.tap(find.text('Open'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 100));
    void expectOpaqueRoute() {
      final backdrop = find.ancestor(
        of: find.text('Detail'),
        matching: find.byType(AppBackground),
      );
      expect(backdrop, findsOneWidget);
      final box = tester.widget<ColoredBox>(
        find.descendant(of: backdrop, matching: find.byType(ColoredBox)).first,
      );
      expect(box.color.a, 1);
      expect(tester.takeException(), isNull);
    }

    expectOpaqueRoute();
    await tester.pumpAndSettle();
    Navigator.of(tester.element(find.text('Detail'))).pop();
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 100));
    expectOpaqueRoute();
    await tester.pumpAndSettle();
    expect(find.text('Detail'), findsNothing);
    expect(find.text('Open'), findsOneWidget);
  });

  testWidgets('balanced floating selection across densities', (tester) async {
    tester.view.physicalSize = const Size(390, 844);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    for (final density in [VisualDensity.standard, VisualDensity.compact]) {
      for (final scale in [1.0, 2.0]) {
        var selected = 2;
        await tester.pumpWidget(
          MaterialApp(
            theme: SailryTheme.of(
              Brightness.dark,
            ).copyWith(visualDensity: density),
            home: MediaQuery(
              data: MediaQueryData(
                size: const Size(390, 844),
                textScaler: TextScaler.linear(scale),
              ),
              child: Scaffold(
                body: Align(
                  alignment: Alignment.bottomCenter,
                  child: StatefulBuilder(
                    builder: (context, update) => FloatingNavigation(
                      selected: selected,
                      onSelected: (value) => update(() => selected = value),
                    ),
                  ),
                ),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        final button = find.byKey(const ValueKey('tab-2'));
        final bounds = tester.getRect(button);
        final dock = tester.getRect(find.byType(Surface));
        final icon = tester.getRect(
          find.descendant(of: button, matching: find.byType(AppIcon)),
        );
        final label = tester.getRect(
          find.descendant(of: button, matching: find.byType(Text)),
        );
        expect(bounds.height, greaterThanOrEqualTo(52));
        expect(
          bounds.top - dock.top,
          closeTo(dock.bottom - bounds.bottom, .01),
        );
        expect(
          icon.top - bounds.top,
          closeTo(bounds.bottom - label.bottom, .01),
        );
        expect(label.top - icon.bottom, closeTo(4, .01));
        expect(icon.width, 20);
        expect(label.left, greaterThanOrEqualTo(bounds.left));
        expect(label.right, lessThanOrEqualTo(bounds.right));
        if (scale == 1) {
          expect(bounds.height, closeTo(52, .01));
          expect(dock.height, closeTo(64, .01));
        } else {
          expect(dock.height, greaterThan(64));
        }
        await tester.tap(find.byKey(const ValueKey('tab-3')));
        await tester.pumpAndSettle();
        expect(selected, 3);
        expect(tester.takeException(), isNull);
      }
    }
  });
}
