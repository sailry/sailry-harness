import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/ui/kit.dart';
import 'package:sailry_mobile/ui/theme.dart';

void main() {
  testWidgets('fills space below page controls', (tester) async {
    tester.view.physicalSize = const Size(390, 844);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    await tester.pumpWidget(
      MaterialApp(
        theme: SailryTheme.of(Brightness.dark),
        home: const PageFrame(
          title: 'Files',
          empty: EmptyState(message: 'No files'),
          child: SizedBox(key: ValueKey('controls'), height: 100),
        ),
      ),
    );
    await tester.pumpAndSettle();
    final empty = find.byType(EmptyState);
    final bounds = tester.getRect(empty);
    final controls = tester.getRect(find.byKey(const ValueKey('controls')));
    final icon = tester.getRect(
      find.descendant(of: empty, matching: find.byType(AppIcon)),
    );
    final label = tester.getRect(find.text('No files'));
    expect(bounds.top, controls.bottom);
    expect(bounds.bottom, 844 - 24);
    expect((icon.top + label.bottom) / 2, closeTo(bounds.center.dy, .01));
    expect(icon.center.dx, closeTo(label.center.dx, .01));
    expect(icon.bottom, lessThan(label.top));
    expect(
      find.ancestor(of: empty, matching: find.byType(Surface)),
      findsNothing,
    );
    expect(tester.takeException(), isNull);
  });

  testWidgets('reachable action with large text and short viewport', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(320, 360);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    var connected = false;
    await tester.pumpWidget(
      MaterialApp(
        theme: SailryTheme.of(Brightness.light),
        builder: (context, child) => MediaQuery(
          data: MediaQuery.of(
            context,
          ).copyWith(textScaler: TextScaler.linear(2)),
          child: child!,
        ),
        home: PageFrame(
          title: 'Hosts',
          empty: EmptyState(
            icon: 'server',
            message: 'Connect a host to start',
            action: FilledButton(
              onPressed: () => connected = true,
              child: const Text('Connect'),
            ),
          ),
          child: const SizedBox(height: 100),
        ),
      ),
    );
    await tester.pumpAndSettle();
    await tester.ensureVisible(find.text('Connect'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Connect'));
    expect(connected, isTrue);
    expect(tester.takeException(), isNull);
  });
}
