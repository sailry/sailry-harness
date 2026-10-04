import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/ui/icons.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('Reicon assets match declared viewports', () async {
    final manifest =
        jsonDecode(await rootBundle.loadString('assets/icons/sources.json'))
            as Map<String, dynamic>;
    final icons = manifest['icons'] as Map<String, dynamic>;
    expect(icons, isNotEmpty);
    for (final name in icons.keys) {
      final picture = await vg.loadPicture(
        SvgAssetLoader('assets/icons/$name.svg'),
        null,
      );
      expect(picture.size, const Size(24, 24), reason: name);
      picture.picture.dispose();
    }
  });

  testWidgets('disclosure follows expansion state', (tester) async {
    await tester.pumpWidget(
      const MaterialApp(
        home: Scaffold(
          body: ExpansionTile(
            title: Text('Details'),
            trailing: DisclosureIcon(),
            children: [Text('Body')],
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    final rotation = find.descendant(
      of: find.byType(DisclosureIcon),
      matching: find.byType(AnimatedRotation),
    );
    expect(tester.widget<AnimatedRotation>(rotation).turns, 0);
    expect(find.text('Body'), findsNothing);
    await tester.tap(find.text('Details'));
    await tester.pumpAndSettle();
    expect(tester.widget<AnimatedRotation>(rotation).turns, .5);
    expect(find.text('Body'), findsOneWidget);
    await tester.tap(find.text('Details'));
    await tester.pumpAndSettle();
    expect(tester.widget<AnimatedRotation>(rotation).turns, 0);
    expect(find.text('Body'), findsNothing);
    expect(find.byType(Icon), findsNothing);
    expect(tester.takeException(), isNull);
  });
}
