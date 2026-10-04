import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/ui/kit.dart';
import 'package:sailry_mobile/ui/theme.dart';

void main() {
  for (final height in <double?>[null, 140]) {
    testWidgets('fill and hit bounds at height $height', (tester) async {
      var taps = 0;
      const surfaceKey = ValueKey('wide-surface');
      await tester.pumpWidget(
        MaterialApp(
          theme: SailryTheme.of(Brightness.light),
          home: Scaffold(
            body: Center(
              child: SizedBox(
                width: 320,
                height: height,
                child: Surface(
                  key: surfaceKey,
                  onTap: () => taps++,
                  child: const Column(
                    mainAxisSize: MainAxisSize.min,
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [Text('Files')],
                  ),
                ),
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();

      final surface = find.byKey(surfaceKey);
      final bounds = tester.getRect(surface);
      expect(bounds.width, 320);
      if (height != null) expect(bounds.height, height);
      for (final type in [Material, Ink, InkWell]) {
        expect(
          tester.getRect(
            find.descendant(of: surface, matching: find.byType(type)),
          ),
          bounds,
          reason: '$type must cover the full surface',
        );
      }
      await tester.tapAt(Offset(bounds.right - 20, bounds.center.dy));
      await tester.pump();
      expect(taps, 1);
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('unconstrained intrinsic size', (tester) async {
    const surfaceKey = ValueKey('intrinsic-surface');
    await tester.pumpWidget(
      MaterialApp(
        theme: SailryTheme.of(Brightness.dark),
        home: const Scaffold(
          body: UnconstrainedBox(
            child: Surface(
              key: surfaceKey,
              child: SizedBox(width: 20, height: 30),
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(tester.getSize(find.byKey(surfaceKey)), const Size(52, 62));
    expect(tester.takeException(), isNull);
  });
}
