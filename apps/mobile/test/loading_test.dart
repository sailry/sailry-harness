import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/ui/kit.dart';
import 'package:sailry_mobile/ui/theme.dart';

void main() {
  testWidgets(
    'long sheets cover the viewport and preserve their scroll position',
    (tester) async {
      tester.view.physicalSize = const Size(390, 844);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      var loading = false;
      late StateSetter update;
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: Builder(
              builder: (context) => TextButton(
                onPressed: () => showAppSheet(
                  context,
                  'Records',
                  scroll: false,
                  child: StatefulBuilder(
                    builder: (context, setState) {
                      update = setState;
                      return LoadingOverlay(
                        loading: loading,
                        scroll: true,
                        child: Column(
                          children: [
                            for (var i = 0; i < 40; i++)
                              SizedBox(height: 60, child: Text('Record $i')),
                          ],
                        ),
                      );
                    },
                  ),
                ),
                child: const Text('Open'),
              ),
            ),
          ),
        ),
      );
      await tester.tap(find.text('Open'));
      await tester.pumpAndSettle();
      final scrollable = find.descendant(
        of: find.byType(LoadingOverlay),
        matching: find.byType(Scrollable),
      );
      await tester.drag(scrollable, const Offset(0, -900));
      await tester.pumpAndSettle();
      final position = tester.state<ScrollableState>(scrollable).position;
      final offset = position.pixels;
      expect(offset, greaterThan(0));
      final viewport = tester.getRect(find.byType(LoadingOverlay));
      expect(viewport.bottom, lessThanOrEqualTo(844));
      update(() => loading = true);
      await tester.pump();
      expect(
        tester.getCenter(find.byType(CircularProgressIndicator)),
        viewport.center,
      );
      await tester.drag(find.byType(LoadingOverlay), const Offset(0, -100));
      await tester.pump();
      expect(position.pixels, offset);
      update(() => loading = false);
      await tester.pumpAndSettle();
      expect(position.pixels, offset);
      expect(tester.takeException(), isNull);
    },
  );

  for (final brightness in Brightness.values) {
    testWidgets(
      'page loading stays centered while preserving navigation in $brightness',
      (tester) async {
        var loading = false;
        var navigations = 0;
        var actions = 0;
        late StateSetter update;
        final scroll = ScrollController(initialScrollOffset: 240);
        addTearDown(scroll.dispose);
        await tester.pumpWidget(
          MaterialApp(
            theme: SailryTheme.of(brightness),
            home: StatefulBuilder(
              builder: (context, setState) {
                update = setState;
                return PageFrame(
                  title: 'Files',
                  loading: loading,
                  scroll: false,
                  actions: [
                    TextButton(
                      onPressed: () => navigations++,
                      child: const Text('Back'),
                    ),
                  ],
                  child: ListView(
                    controller: scroll,
                    children: [
                      const SizedBox(height: 260),
                      TextButton(
                        onPressed: () => actions++,
                        child: const Text('Open file'),
                      ),
                      const SizedBox(height: 1200),
                    ],
                  ),
                );
              },
            ),
          ),
        );
        final position = tester.getCenter(find.text('Open file'));
        final viewport = tester.getRect(find.byType(LoadingOverlay));
        update(() => loading = true);
        await tester.pump();
        expect(
          tester.getCenter(find.byType(CircularProgressIndicator)),
          viewport.center,
        );
        await tester.tapAt(position);
        await tester.drag(find.byType(LoadingOverlay), const Offset(0, -100));
        await tester.pump();
        expect(actions, 0);
        expect(scroll.offset, 240);
        await tester.tap(find.text('Back'));
        expect(navigations, 1);
        update(() => loading = false);
        await tester.pumpAndSettle();
        expect(find.byType(CircularProgressIndicator), findsNothing);
        expect(tester.getRect(find.byType(LoadingOverlay)), viewport);
        await tester.tap(find.text('Open file'));
        expect(actions, 1);
        expect(tester.takeException(), isNull);
      },
    );
  }

  testWidgets(
    'a sheet-sized overlay retains drafts and hides blocked semantics',
    (tester) async {
      var loading = false;
      late StateSetter update;
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: SingleChildScrollView(
              child: StatefulBuilder(
                builder: (context, setState) {
                  update = setState;
                  return LoadingOverlay(
                    loading: loading,
                    minHeight: 128,
                    child: const TextField(
                      decoration: InputDecoration(labelText: 'Draft'),
                    ),
                  );
                },
              ),
            ),
          ),
        ),
      );
      await tester.enterText(find.byType(TextField), 'Keep this draft');
      update(() => loading = true);
      await tester.pump();
      expect(find.bySemanticsLabel(tr('loading')), findsOneWidget);
      expect(find.semantics.byLabel('Draft'), findsNothing);
      expect(
        tester
            .widget<EditableText>(find.byType(EditableText))
            .focusNode
            .hasFocus,
        isFalse,
      );
      final container = tester.getRect(find.byType(LoadingOverlay));
      expect(
        tester.getCenter(find.byType(CircularProgressIndicator)),
        container.center,
      );
      update(() => loading = false);
      await tester.pumpAndSettle();
      expect(find.text('Keep this draft'), findsOneWidget);
      expect(find.semantics.byLabel('Draft'), findsOneWidget);
      expect(tester.takeException(), isNull);
    },
  );
}
