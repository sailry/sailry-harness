import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/content/markdown.dart';
import 'package:sailry_mobile/ui/theme.dart';

void main() {
  for (final brightness in Brightness.values) {
    group(brightness.name, () {
      final theme = SailryTheme.of(brightness);
      final link = SailryTheme.link(brightness);

      test('links take the theme link color', () {
        final style = contentStyleSheet(theme);
        expect(style.a?.color, link);
        expect(style.p?.color, isNot(link));
      });

      test('muted text keeps the link color', () {
        final style = contentStyleSheet(theme, muted: true);
        expect(style.a?.color, link);
        expect(style.p?.color, theme.colorScheme.onSurfaceVariant);
      });

      test('code inside a link keeps the link color', () {
        final style = contentStyleSheet(theme);
        expect(style.p!.merge(style.a).merge(style.code).color, link);
        expect(style.p!.merge(style.code).color, style.p?.color);
      });
    });
  }
}
